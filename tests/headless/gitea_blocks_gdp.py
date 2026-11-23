"""Check GPUI issue blocks against an isolated, disposable loopback Gitea fork.

Set GITEA_URL and GITEA_TOKEN for the disposable server before running.
The selected fixture issue is briefly closed and a new issue is created.
"""

import argparse
import ipaddress
import json
import os
from pathlib import Path
import re
import tempfile
import time
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener
from uuid import uuid4

from editor_gdp import TEXT_MODIFIER, launch


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, _request, _fp, _code, _message, _headers, _new_url):
        return None


def local_origin():
    raw_origin = os.environ["GITEA_URL"].rstrip("/")
    parsed = urlsplit(raw_origin)
    if (
        parsed.scheme not in {"http", "https"}
        or parsed.hostname is None
        or not ipaddress.ip_address(parsed.hostname).is_loopback
        or not parsed.port
        or parsed.username
        or parsed.password
        or parsed.path
        or parsed.query
        or parsed.fragment
    ):
        raise ValueError("Use the origin of a disposable loopback Gitea server with an explicit port")
    return raw_origin


def issue_api(origin, repository, number, *, state=None):
    payload = json.dumps({"state": state}).encode() if state is not None else None
    request = Request(
        f"{origin}/api/v1/repos/{repository}/issues/{number}",
        data=payload,
        method="PATCH" if payload is not None else "GET",
        headers={
            "Authorization": f'token {os.environ["GITEA_TOKEN"]}',
            "Content-Type": "application/json",
        },
    )
    with build_opener(ProxyHandler({}), NoRedirect()).open(request, timeout=10) as response:
        return json.load(response)


def eventually(editor, read, predicate, message):
    deadline = time.monotonic() + 15
    value = None
    while time.monotonic() < deadline:
        editor.advance(50)
        value = read()
        if predicate(value):
            return value
        time.sleep(0.05)
    raise AssertionError(f"{message}: {value!r}")


def check_blocks(binary, origin, repository, number, fixture):
    issue = issue_api(origin, repository, number)
    assert issue["state"] == "open", "The disposable fixture issue must start open"
    url = f"{origin}/{repository}/issues/{number}"
    title = f"GPUI Blocks acceptance {uuid4().hex[:12]}"
    body = f"{title}\nSecond line verifies the source block was sent."
    fixture.write_text(f"{body}\n", encoding="utf-8")

    os.environ["TERRAPHIM_GITEA_REPO"] = repository
    with launch(binary, fixture) as editor:
        editor.press(f"{TEXT_MODIFIER}-a")
        editor.type(f"{url}\n\n{body}\n")
        original = editor.document()
        assert url in original and body in original, original
        editor.press("ctrl-shift-b")
        editor.call("bounds", id="gitea-card-0")
        eventually(
            editor,
            lambda: editor.call("gitea_cards"),
            lambda cards: any(
                card.get("title") == issue["title"] and card.get("state") == "open"
                for card in cards
            ),
            "Gitea issue did not load in Blocks",
        )
        editor.click("gitea-details-0")
        assert any(
            card.get("expanded") and card.get("body") == issue["body"]
            for card in editor.call("gitea_cards")
        )
        editor.click("gitea-details-0")

        closed = issue_api(origin, repository, number, state="closed")
        try:
            assert closed["state"] == "closed"
            editor.click("gitea-refresh-0")
            eventually(
                editor,
                lambda: editor.call("gitea_cards"),
                lambda cards: any(
                    card.get("number") == number and card.get("state") == "closed"
                    for card in cards
                ),
                "Refreshing the card did not show the server-side state change",
            )
            assert editor.document() == original, "Refreshing modified the Markdown document"
        finally:
            issue_api(origin, repository, number, state="open")

        editor.click("create-gitea-issue-1")
        assert editor.document() == original, "Opening the issue draft modified the document"
        editor.click("submit-gitea-issue")
        linked = eventually(
            editor,
            editor.document,
            lambda document: document.count(f"{origin}/{repository}/issues/") == 2,
            "The newly created issue was not linked in the document",
        )
        (created_number,) = [
            int(index)
            for index in re.findall(rf"{re.escape(origin)}/{re.escape(repository)}/issues/(\d+)", linked)
            if int(index) != number
        ]
        created = issue_api(origin, repository, created_number)
        assert created["title"] == title, created
        assert created["body"].startswith(body), created
        assert original.rstrip("\n") in linked
        editor.call("bounds", id="gitea-card-2")
        eventually(
            editor,
            lambda: editor.call("gitea_cards"),
            lambda cards: any(
                card.get("number") == created_number and card.get("title") == title
                for card in cards
            ),
            "The new issue link did not render a live card",
        )

        editor.press("ctrl-shift-b")
        editor.press(f"{TEXT_MODIFIER}-z")
        undone = editor.document()
        assert undone == original, f"Undo expected {original!r}, got {undone!r}"
        assert issue_api(origin, repository, created_number)["state"] == "open"
    print("PASS Gitea issue link, details, state refresh, creation from a block, and local undo")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="Terraphim Editor compiled with --features gdp")
    parser.add_argument("--repository", required=True, help="Disposable owner/repository")
    parser.add_argument("--issue", type=int, required=True, help="Existing disposable open issue number")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repository) or args.issue < 1:
        parser.error("use a valid owner/repository and a positive issue number")
    binary = args.binary.resolve(strict=True)
    origin = local_origin()
    with tempfile.TemporaryDirectory(prefix="terraphim-gitea-gdp-") as directory:
        check_blocks(binary, origin, args.repository, args.issue, Path(directory) / "gitea.md")


if __name__ == "__main__":
    main()
