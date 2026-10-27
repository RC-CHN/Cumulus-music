"""Independent evidence audit; reads artifacts and uses FFmpeg, never the API."""
import hashlib
import json
from pathlib import Path
import sqlite3
import statistics
import subprocess
import sys
import urllib.parse
import urllib.request


def main(root: Path, remote: bool = False):
    load = lambda p: json.loads(p.read_text())
    plan = load(root / "plan.json")
    report = load(root / "report.json")
    bound = load(root / "binding.json")
    cleanup = load(root / "evidence/cleanup.json")
    assert report["operation_ok"] and report["cleanup_ok"] and cleanup["ok"]
    requests = []
    for path in sorted((root / "evidence").glob("api-*-request.json")):
        request = load(path)
        reply = load(path.with_name(path.name.replace("request", "response")))
        assert reply["code"] == 200, (request["path"], reply.get("code"))
        requests.append((request, reply))
    routes = [request["path"] for request, _ in requests]
    mutation_routes = ["/api/playlist/create", "/api/nos/token/alloc", "/api/playlist/cover/update", "/api/playlist/delete"]
    for route in mutation_routes:
        assert routes.count(route) == 1, route
    assert [p for p in routes if p in mutation_routes] == mutation_routes
    assert set(routes) <= set(mutation_routes + ["/api/w/nuser/account/get", "/api/user/playlist", "/api/v6/playlist/detail"])

    one = lambda route: next(pair for pair in requests if pair[0]["path"] == route)
    created_request, created_reply = one("/api/playlist/create")
    pid = str(created_reply.get("id") or created_reply["playlist"]["id"])
    assert pid not in plan["ids"]
    assert created_request["payload"]["name"] == plan["name"]
    assert created_request["payload"]["privacy"] == 10
    allocation_request, allocation_reply = one("/api/nos/token/alloc")
    assert allocation_request["payload"] == dict(filename="cover.jpg", ext="jpg", type="image", bucket="yyimgs")
    allocation = allocation_reply.get("data") or allocation_reply["result"]
    document = str(allocation["docId"])
    assert allocation["bucket"] == "yyimgs"
    assert one("/api/playlist/cover/update")[0]["payload"] == dict(id=pid, coverImgId=document)
    assert one("/api/playlist/delete")[0]["payload"] == dict(pid=pid)
    assert str(bound["after"]["cover_id"]) == document
    assert bound["after"]["tracks"] == bound["before"]["tracks"] == []
    assert bound["before"]["name"] == bound["after"]["name"] == plan["name"]
    assert bound["before"]["private"] and bound["after"]["private"]

    cycles, current = [], []
    for request, reply in requests:
        if request["path"] != "/api/user/playlist":
            continue
        if int(request["payload"].get("offset", 0)) == 0:
            current = []
        current.extend(str(p["id"]) for p in reply["playlist"])
        if not reply.get("more", False):
            cycles.append(current[:])
    assert cycles[0] == cycles[-1] == plan["ids"] == cleanup["ids"]
    task_id = report["operation"]["task_id"]
    db = sqlite3.connect(f"file:{root.resolve()}/core.sqlite3?mode=ro", uri=True)
    def record(kind, key):
        return json.loads(db.execute("SELECT value FROM state WHERE owner=? AND kind=? AND key=?", (plan["owner"], kind, key)).fetchone()[0])
    task = record("playlist-covers-v1", task_id)
    child = record("nos_uploads_v1", task_id)
    mutation = record("playlist-mutations", task["binding_id"])
    assert task["phase"] == "confirmed" and task["source"] == task["encoded"] == ""
    assert task["allocation"]["resource"] == child["allocation"]["resource"] == document
    assert child["allocation"]["key"] == allocation["objectKey"]
    assert child["status"] == "uploaded" and child["pending"] is None
    assert child["offset"] == child["source"]["bytes"] == task["total"]
    assert mutation["phase"] == "confirmed"
    assert mutation["command"] == dict(kind="cover", id=pid, revision=task["revision"], document_id=document)
    jpeg = (root / "uploaded.jpg").read_bytes()
    digest = hashlib.sha256(jpeg).hexdigest()
    assert digest == task["sha256"] == child["source"]["sha256"]
    assert hashlib.md5(jpeg).hexdigest() == task["md5"] == child["source"]["md5"]
    assert len(jpeg) == child["offset"]
    remote_verified = False
    remote_identical = False
    if remote:
        url = one("/api/playlist/cover/update")[1]["url"]
        def public_url(url):
            parsed = urllib.parse.urlsplit(url)
            assert parsed.hostname and parsed.hostname.endswith(".music.126.net")
            assert parsed.scheme in ("https", "http") and not parsed.username and not parsed.password
            assert parsed.port is None and not parsed.query and not parsed.fragment
            return urllib.parse.urlunsplit(parsed._replace(scheme="https"))
        class PublicRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, req, fp, code, msg, headers, newurl):
                return super().redirect_request(req, fp, code, msg, headers, public_url(newurl))
        opener = urllib.request.build_opener(PublicRedirect())
        with opener.open(public_url(url), timeout=30) as response:
            content = response.read(16 * 1024 * 1024 + 1)
        (root / "remote-cover.jpg").write_bytes(content)
        remote_identical = content == jpeg
        (root / "remote-cover-metadata.json").write_text(json.dumps(dict(bytes=len(content),
            sha256=hashlib.sha256(content).hexdigest(), identical=remote_identical),indent=2)+"\n")
    assert not (root / f"playlist-cover-{task_id}.jpg").exists()
    def rgb(path):
        return subprocess.run(["ffmpeg", "-v", "error", "-i", str(path), "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"], check=True, capture_output=True).stdout
    source, cropped = rgb(root / "synthetic-input.png"), rgb(root / "uploaded.jpg")
    assert len(source) == 640 * 480 * 3 and len(cropped) == 320 * 320 * 3
    expected = b"".join(source[((y+40)*640+320)*3:((y+40)*640+640)*3] for y in range(320))
    errors = [abs(a-b) for a,b in zip(expected,cropped)]
    mean_error = statistics.mean(errors)
    assert mean_error < 3, mean_error
    remote_mean_error = None
    if remote:
        served = rgb(root / "remote-cover.jpg")
        assert len(served) == len(cropped), "public image dimensions differ"
        remote_mean_error = statistics.mean(abs(a-b) for a,b in zip(cropped,served))
        assert remote_mean_error < 3, remote_mean_error
        remote_verified = True
    result = dict(ok=True, api_requests=len(requests), isolated_mutations=4,
        full_order_restored=True, baseline_count=len(plan["ids"]), exact_document_binding=True,
        upload_bytes=len(jpeg), sha256=digest, independent_decoder="ffmpeg",
        crop_width=320, crop_height=320, crop_rgb_mean_error=mean_error,
        native_local_cleanup=True, remote_image_deleted=False, image_orphan_possible=True,
        public_image_verified=remote_verified, public_object_identical_bytes=remote_identical,
        public_image_rgb_mean_error=remote_mean_error, ui_tested=False)
    (root / "independent-audit.json").write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n")
    print(json.dumps(result,ensure_ascii=False))


if __name__ == "__main__":
    main(Path(sys.argv[1]), "--remote" in sys.argv[2:])
