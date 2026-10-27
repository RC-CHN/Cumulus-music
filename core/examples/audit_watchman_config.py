#!/usr/bin/env python3
"""Independently inspect the anonymous Rust configuration proof; no network."""
import argparse
import json
import sqlite3
from pathlib import Path
from urllib.parse import parse_qs, urlsplit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    report = json.loads((root / "report.json").read_text())
    assert report["ok"] and report["reopenUsedCache"] and report["revalidated"]
    assert report["deviceOrBehaviorReports"] == report["musicBusinessRequests"] == 0
    assert report["executedRemoteScripts"] is False
    count = report["requests"]
    assert count >= 2
    assert len(list(root.glob("request-*.json"))) == count
    responses = []
    queries = []
    for n in range(count):
        request = json.loads((root / f"request-{n}.json").read_text())
        url = urlsplit(request["url"])
        assert url.scheme == "https" and url.netloc in {
            "ac.dun.163.com", "ac.dun.163yun.com"
        }
        assert url.path == "/v2/config/js" and not url.fragment
        assert request["method"] == "GET"
        assert request["cookie"] is request["fingerprint"] is False
        query = parse_qs(url.query, keep_blank_values=True, strict_parsing=True)
        assert set(query) == {"pn", "cvk", "cb", "t"}
        assert all(len(v) == 1 for v in query.values())
        assert query["pn"] == ["YD00000558929251"]
        assert query["cb"][0].startswith("__wmjsonp_")
        assert int(query["t"][0]) > 0
        queries.append(query)
        response_file = root / f"response-{n}.json"
        if response_file.exists():
            responses.append(json.loads(response_file.read_text()))
    assert len(responses) == 2  # first load and forced revalidation, no cache GET
    assert queries[0]["cvk"] == [""]
    assert responses[0]["code"] == 200
    assert responses[1]["code"] in (200, 304)
    assert queries[-1]["cvk"] == [responses[0]["result"]["cvk"]]
    expected = responses[0] if responses[1]["code"] == 304 else responses[1]
    database = root / "state/core.sqlite3"
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as db:
        rows = db.execute("SELECT owner, kind, key, value FROM state").fetchall()
    assert len(rows) == 1
    owner, kind, key, raw = rows[0]
    assert (owner, kind, key) == ("device", "watchman-config-v1", "YD00000558929251")
    state = json.loads(raw)
    assert state["revision"] == 2
    assert state["raw"] == expected
    assert state["raw"]["result"]["v"] == report["sdkBuild"]
    assert report["sdkBuild"] in {"2.7.5_af2952a4", "2.7.5_602a5ad7"}
    ttl = expected["result"]["ivp"]
    assert state["expires_ms"] >= int(queries[-1]["t"][0]) + ttl
    assert state["expires_ms"] <= int(database.stat().st_mtime * 1000) + ttl + 1000
    result = dict(ok=True, requests=count, responseCodes=[r["code"] for r in responses],
                  sdkBuild=report["sdkBuild"], revision=state["revision"],
                  storedResponseMatches=True, onlyConfigurationState=True,
                  networkRequestsByAudit=0)
    with (root / "independent-audit.json").open("x") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
