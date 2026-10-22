"""The check of step S0 for compare.py: ``python -m pytest spike/test_compare.py -q``."""
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from compare import diff  # noqa: E402

A = {"layers": [{"id": "a", "paint": {"w": 1.0, "c": "#fff"}}], "n": None}


def test_equal_to_itself():
    assert diff(A, json.loads(json.dumps(A))) == []


def test_tolerance_and_order_of_keys():
    b = {"n": None, "layers": [{"paint": {"c": "#fff", "w": 1.0 + 1e-12}, "id": "a"}]}
    assert diff(A, b) == []
    assert [d[0] for d in diff(A, {**b, "layers": [{"id": "a", "paint": {"c": "#fff", "w": 1.001}}]})] == ["layers[0].paint.w"]


def test_differences():
    assert [d[0] for d in diff(A, {**A, "layers": []})] == ["layers.length"]
    assert [d[0] for d in diff(A, {"layers": A["layers"]})] == ["n"]
    assert diff({"x": 1}, {"x": True})              # a bool is not the number 1


def test_golden_file_against_itself_and_a_changed_one(tmp_path):
    g = Path(__file__).parent / "data" / "golden.json"
    if not g.exists():
        return                                       # run tools/make_spike_fixture.py first
    run = lambda *a: subprocess.run([sys.executable, str(Path(__file__).parent / "compare.py"), *a], capture_output=True, text=True)   # noqa: E731
    assert run(str(g), str(g)).stdout.strip() == "equal"
    s = json.loads(g.read_text())
    s["sources"]["simple"]["data"]["features"][812]["properties"]["__rs_s"] += 1
    (tmp_path / "x.json").write_text(json.dumps(s))
    r = run(str(g), str(tmp_path / "x.json"))
    assert r.returncode == 1 and "sources.simple.data.features[812].properties.__rs_s" in r.stdout
