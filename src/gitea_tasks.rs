//! Gitea issue references and the local gitea-robot provider for Blocks.

use anyhow::{bail, ensure, Context, Result};
use markdown::{mdast::Node, ParseOptions};
use serde::Deserialize;
use std::{
    env,
    ffi::OsStr,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, Write},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime},
};
use url::Url;

mod tree;
pub(crate) use tree::TaskTree;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(35);
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct IssueRef {
    pub(crate) owner: String,
    pub(crate) repo: String,
    pub(crate) number: u64,
}

impl IssueRef {
    pub(crate) fn parse(text: &str, origin: &str) -> Option<Self> {
        let base = trusted_base(origin)?;
        let url = Url::parse(text.trim()).ok()?;
        if url.origin() != base.origin() || !url.username().is_empty() || url.password().is_some() {
            return None;
        }
        let relative = url.path().strip_prefix(base.path().trim_end_matches('/'))?;
        let path = relative.strip_prefix('/')?;
        let mut parts = path.strip_suffix('/').unwrap_or(path).split('/');
        let owner = parts.next().filter(|part| valid_repository_part(part))?;
        let repo = parts.next().filter(|part| valid_repository_part(part))?;
        if parts.next()? != "issues" {
            return None;
        }
        let number = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        let number = positive_issue_number(number)?;
        Some(Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            number,
        })
    }

    pub(crate) fn in_block(text: &str, origin: &str) -> Option<Self> {
        if origin.is_empty() || !text.contains("/issues/") {
            return None;
        }
        let root = markdown::to_mdast(text, &ParseOptions::gfm()).ok()?;
        Self::in_node(&root, origin)
    }

    fn in_node(node: &Node, origin: &str) -> Option<Self> {
        match node {
            Node::Link(link) => Self::parse(&link.url, origin),
            Node::Text(text) => Self::parse(&text.value, origin),
            _ => node
                .children()?
                .iter()
                .find_map(|child| Self::in_node(child, origin)),
        }
    }

    pub(crate) fn url(&self, origin: &str) -> String {
        format!(
            "{}/{}/{}/issues/{}",
            origin.trim_end_matches('/'),
            self.owner,
            self.repo,
            self.number
        )
    }
}

fn positive_issue_number(value: &str) -> Option<u64> {
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number = value.parse::<i64>().ok()?;
    u64::try_from(number).ok().filter(|number| *number > 0)
}

fn valid_repository_part(name: &str) -> bool {
    !name.is_empty()
        && !matches!(name, "." | "..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn repository_parts(repository: &str) -> Option<(&str, &str)> {
    let (owner, repo) = repository.trim().split_once('/')?;
    (valid_repository_part(owner) && valid_repository_part(repo)).then_some((owner, repo))
}

fn trusted_base(value: &str) -> Option<Url> {
    let url = Url::parse(value.trim()).ok()?;
    let valid = matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    valid.then_some(url)
}

#[derive(Clone)]
pub(crate) struct GiteaTasks {
    origin: String,
    default_repository: String,
    executable: PathBuf,
}

impl GiteaTasks {
    pub(crate) fn from_env() -> Self {
        let origin = env::var("GITEA_URL")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(configured_origin)
            .and_then(|value| trusted_base(&value))
            .map(|url| url.as_str().trim_end_matches('/').to_owned())
            .unwrap_or_default();
        Self {
            origin,
            default_repository: env::var("TERRAPHIM_GITEA_REPO").unwrap_or_default(),
            executable: gtr_path(),
        }
    }

    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }

    pub(crate) fn default_repository(&self) -> &str {
        &self.default_repository
    }

    pub(crate) fn view(&self, issue: &IssueRef) -> Result<IssueSnapshot> {
        ensure!(
            valid_repository_part(&issue.owner)
                && valid_repository_part(&issue.repo)
                && issue.number > 0
                && i64::try_from(issue.number).is_ok(),
            "Invalid Gitea issue reference"
        );
        let output = self.run([
            OsStr::new("view-issue"),
            OsStr::new("--owner"),
            OsStr::new(&issue.owner),
            OsStr::new("--repo"),
            OsStr::new(&issue.repo),
            OsStr::new("--issue"),
            OsStr::new(&issue.number.to_string()),
            OsStr::new("--json"),
        ])?;
        read_snapshot(
            &output.stdout,
            output.cached_at.as_deref(),
            issue.number,
            SystemTime::now(),
        )
    }

    pub(crate) fn create(&self, repository: &str, title: &str, body: &str) -> Result<IssueRef> {
        let (owner, repo) =
            repository_parts(repository).context("Enter a Gitea repository as owner/repo")?;
        let title = title.trim();
        ensure!(
            !title.is_empty() && !title.contains(['\r', '\n']),
            "Enter a single-line issue title"
        );
        let mut body_file = tempfile::NamedTempFile::new().context("Prepare Gitea issue body")?;
        body_file.write_all(body.as_bytes())?;
        let output = self
            .run([
                OsStr::new("create-issue"),
                OsStr::new("--owner"),
                OsStr::new(owner),
                OsStr::new("--repo"),
                OsStr::new(repo),
                OsStr::new("--title"),
                OsStr::new(title),
                OsStr::new("--body-file"),
                body_file.path().as_os_str(),
            ])
            .map_err(|error| {
                if error.is::<CommandUncertain>() {
                    CreateUncertain(error.to_string()).into()
                } else {
                    error
                }
            })?;
        let number = created_issue_number(&output.stdout)
            .ok_or_else(|| CreateUncertain("Gitea creation could not be confirmed".into()))?;
        Ok(IssueRef {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            number,
        })
    }

    fn run<const N: usize>(&self, args: [&OsStr; N]) -> Result<GtrOutput> {
        ensure!(
            !self.origin.is_empty(),
            "Configure GITEA_URL with a valid Gitea base URL"
        );
        let mut output = tempfile::tempfile().context("Prepare Gitea command output")?;
        let mut diagnostic = tempfile::tempfile().context("Prepare Gitea command diagnostics")?;
        let mut child = Command::new(&self.executable)
            .args(args)
            .env("GITEA_URL", &self.origin)
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(diagnostic.try_clone()?)
            .spawn()
            .context("Start gtr. Make sure gitea-robot is installed")?;

        let deadline = Instant::now() + COMMAND_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let detail = match result {
                        Err(_) => "Could not confirm gtr completion",
                        _ => "Gitea request timed out",
                    };
                    return Err(CommandUncertain(detail.to_owned()).into());
                }
            }
        };

        let mut error = String::new();
        let _ = diagnostic
            .rewind()
            .and_then(|()| diagnostic.take(4096).read_to_string(&mut error));
        if !status.success() {
            let first_line = error.lines().next().unwrap_or_default();
            if first_line == "Error: GITEA_TOKEN environment variable required" {
                bail!("Configure GITEA_TOKEN for gtr");
            }
            if first_line.starts_with("Error: GITEA_NOSTR_KEY is invalid:") {
                bail!("The Gitea Nostr signing configuration is invalid");
            }
            let status_line = first_line.strip_prefix("Error: error: ");
            match status_line {
                Some("401 Unauthorized" | "403 Forbidden") => {
                    bail!("Gitea denied access. Check your configured credentials and repository permissions");
                }
                Some("404 Not Found") => bail!("Gitea could not find the issue or repository"),
                Some(
                    "400 Bad Request" | "422 Unprocessable Entity" | "422 Unprocessable Content",
                ) => {
                    bail!("Gitea rejected the issue details");
                }
                _ => {}
            }
            return Err(CommandUncertain(format!(
                "gtr failed ({status}). Verify the Gitea connection and repository"
            ))
            .into());
        }

        let mut response = String::new();
        output
            .rewind()
            .and_then(|()| {
                output
                    .take(MAX_RESPONSE_BYTES + 1)
                    .read_to_string(&mut response)
            })
            .map_err(|_| CommandUncertain("Could not read the gtr response".into()))?;
        if response.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(
                CommandUncertain("Gitea response exceeds the supported size".into()).into(),
            );
        }
        Ok(GtrOutput {
            stdout: response,
            cached_at: cache_time_from_diagnostic(&error).map(str::to_owned),
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct CommandUncertain(String);

#[derive(Debug, thiserror::Error)]
#[error("{0}. Check Gitea for the issue before retrying")]
pub(crate) struct CreateUncertain(String);

struct GtrOutput {
    stdout: String,
    cached_at: Option<String>,
}

fn cache_time_from_diagnostic(diagnostic: &str) -> Option<&str> {
    diagnostic.lines().find_map(|line| {
        let (_, value) = line
            .strip_prefix("Warning: ")?
            .rsplit_once(", fetched_at ")?;
        chrono::DateTime::parse_from_rfc3339(value).ok()?;
        Some(value)
    })
}

fn gtr_path() -> PathBuf {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .map(|path| path.join("gtr"))
        .chain(env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/bin/gtr")))
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("gtr"))
}

fn configured_origin() -> Option<String> {
    let path = PathBuf::from(env::var_os("HOME")?).join(".config/terraphim/gitea.env");
    let config = BufReader::new(File::open(path).ok()?);
    config
        .lines()
        .map_while(std::result::Result::ok)
        .find_map(|line| {
            let key_value = line.trim().strip_prefix("export ").unwrap_or(line.trim());
            let (key, value) = key_value.split_once('=')?;
            if key.trim() != "GITEA_URL" {
                return None;
            }
            let value = value.trim();
            Some(
                value
                    .strip_prefix('"')
                    .and_then(|inner| inner.strip_suffix('"'))
                    .or_else(|| {
                        value
                            .strip_prefix('\'')
                            .and_then(|inner| inner.strip_suffix('\''))
                    })
                    .unwrap_or(value)
                    .to_owned(),
            )
        })
}

#[derive(Clone)]
pub(crate) struct IssueSnapshot {
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) state: String,
    pub(crate) labels: Vec<String>,
    pub(crate) assignees: Vec<String>,
    pub(crate) fetched_at: SystemTime,
    pub(crate) stale: bool,
}

#[derive(Deserialize)]
struct Envelope {
    v: u32,
    ok: bool,
    result: Option<GtrIssue>,
}

#[derive(Deserialize)]
struct GtrIssue {
    number: u64,
    title: String,
    body: String,
    state: String,
    labels: Option<Vec<Label>>,
    assignees: Option<Vec<User>>,
    assignee: Option<User>,
    pull_request: Option<serde_json::Value>,
    #[serde(default)]
    stale: bool,
    fetched_at: Option<String>,
}

#[derive(Deserialize)]
struct Label {
    name: String,
}

#[derive(Deserialize)]
struct User {
    login: String,
}

fn read_snapshot(
    output: &str,
    cached_at: Option<&str>,
    expected_issue: u64,
    now: SystemTime,
) -> Result<IssueSnapshot> {
    let envelope: Envelope = serde_json::from_str(output).context("Read gtr issue envelope")?;
    ensure!(
        envelope.v == 1 && envelope.ok,
        "gtr did not return an issue"
    );
    let issue = envelope
        .result
        .context("gtr response is missing its issue")?;
    ensure!(
        issue.number == expected_issue,
        "gtr returned a different issue"
    );
    ensure!(
        issue.pull_request.is_none(),
        "This reference resolves to a pull request"
    );
    let stale = issue.stale || cached_at.is_some();
    let fetched_at = if stale {
        let timestamp = issue
            .fetched_at
            .as_deref()
            .or(cached_at)
            .context("Cached Gitea issue is missing its fetch time")?;
        SystemTime::from(
            chrono::DateTime::parse_from_rfc3339(timestamp)
                .context("Read cached Gitea issue fetch time")?,
        )
    } else {
        now
    };
    let mut assignees: Vec<_> = issue
        .assignees
        .unwrap_or_default()
        .into_iter()
        .map(|user| user.login)
        .collect();
    if let Some(user) = issue.assignee {
        if !assignees.contains(&user.login) {
            assignees.push(user.login);
        }
    }

    Ok(IssueSnapshot {
        title: issue.title,
        body: issue.body,
        state: issue.state,
        labels: issue
            .labels
            .unwrap_or_default()
            .into_iter()
            .map(|label| label.name)
            .collect(),
        assignees,
        fetched_at,
        stale,
    })
}

fn created_issue_number(output: &str) -> Option<u64> {
    #[derive(Deserialize)]
    struct CreatedIssue {
        number: u64,
    }

    if let Some(number) = output
        .lines()
        .next()?
        .strip_prefix("Created issue #")
        .and_then(|line| line.split_once(':'))
    {
        return positive_issue_number(number.0);
    }
    let created: CreatedIssue = serde_json::from_str(output).ok()?;
    (created.number > 0 && i64::try_from(created.number).is_ok()).then_some(created.number)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://git.terraphim.cloud";

    #[test]
    fn links_round_trip_and_only_resolve_on_the_configured_server() {
        let url = format!("{ORIGIN}/my-org/private.repo/issues/42");
        let issue = IssueRef::parse(&url, ORIGIN).unwrap();
        assert_eq!(issue.url(ORIGIN), url);
        assert_eq!(
            IssueRef::in_block(&format!("See [task]({url})."), ORIGIN),
            Some(issue.clone())
        );
        assert_eq!(
            IssueRef::in_block(&format!("See {url}."), ORIGIN),
            Some(issue)
        );
        assert!(IssueRef::in_block(&format!("`{url}`"), ORIGIN).is_none());
        assert!(IssueRef::in_block(&format!("[`{url}`](https://example.org)"), ORIGIN).is_none());
        assert!(
            IssueRef::parse("https://git.terraphim.cloud.example/o/r/issues/42", ORIGIN).is_none()
        );
        assert!(IssueRef::parse("https://x@git.terraphim.cloud/o/r/issues/42", ORIGIN).is_none());
        assert!(IssueRef::parse(&format!("{ORIGIN}/o/r/pulls/42"), ORIGIN).is_none());
        assert!(IssueRef::parse(&format!("{ORIGIN}/o/r/issues/0"), ORIGIN).is_none());
        assert!(IssueRef::parse(&format!("{ORIGIN}/o/%2Fr/issues/42"), ORIGIN).is_none());
        assert!(IssueRef::parse(&url, &format!("{ORIGIN}/gitea")).is_none());
        let nested = format!("{ORIGIN}/gitea/o/r/issues/42");
        assert_eq!(
            IssueRef::parse(&nested, &format!("{ORIGIN}/gitea"))
                .unwrap()
                .number,
            42
        );
    }

    #[test]
    fn parses_real_gtr_envelope_and_preserves_cache_fetch_time() {
        let output = r#"{"v":1,"ok":true,"result":{"number":42,"title":"Résumé","body":"Steps","state":"open","labels":[{"name":"status/blocked"}],"assignee":{"login":"alex"},"assignees":[{"login":"alex"},{"login":"bot"}],"pull_request":null,"stale":true,"fetched_at":"2026-11-23T10:21:27Z"}}"#;
        let snapshot = read_snapshot(output, None, 42, SystemTime::now()).unwrap();
        assert_eq!(snapshot.title, "Résumé");
        assert_eq!(snapshot.body, "Steps");
        assert_eq!(snapshot.state, "open");
        assert_eq!(snapshot.labels, ["status/blocked"]);
        assert_eq!(snapshot.assignees, ["alex", "bot"]);
        assert!(snapshot.stale);
        assert_eq!(
            snapshot.fetched_at,
            SystemTime::from(chrono::DateTime::parse_from_rfc3339("2026-11-23T10:21:27Z").unwrap())
        );
        assert!(read_snapshot(output, None, 43, SystemTime::now()).is_err());
        let pull = output.replace("\"pull_request\":null", "\"pull_request\":{}");
        assert!(read_snapshot(&pull, None, 42, SystemTime::now()).is_err());
    }

    #[test]
    fn reads_legacy_gtr_cache_timestamp_without_retaining_other_diagnostics() {
        let output = r#"{"v":1,"ok":true,"result":{"number":42,"title":"Example","body":"","state":"closed","labels":null,"assignees":null}}"#;
        let time =
            cache_time_from_diagnostic("Warning: offline cache, fetched_at 2026-11-23T10:21:27Z\n");
        assert_eq!(time, Some("2026-11-23T10:21:27Z"));
        let snapshot = read_snapshot(output, time, 42, SystemTime::now()).unwrap();
        assert!(snapshot.stale);
        assert_eq!(snapshot.labels, Vec::<String>::new());
        assert_eq!(snapshot.assignees, Vec::<String>::new());
        assert!(cache_time_from_diagnostic("Warning: offline cache, fetched_at invalid").is_none());
    }

    #[test]
    fn parses_gtr_create_result_without_using_title_as_issue_identity() {
        assert_eq!(
            created_issue_number("Created issue #42: #1: Please fix\n"),
            Some(42)
        );
        assert_eq!(created_issue_number("{\"number\":42}"), Some(42));
        assert_eq!(created_issue_number("Some text mentioned #42"), None);
        assert_eq!(created_issue_number("Created issue #0: zero"), None);
        assert_eq!(repository_parts("org/project"), Some(("org", "project")));
        assert!(repository_parts("org/project/other").is_none());
        assert!(repository_parts("org/..").is_none());
    }
}
