"""The check of step S0 for compare.py: ``python -m pytest spike/test_compare.py -q``."""
import json
import shutil
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


def test_rust_style_equals_golden(tmp_path):
    """S1 to S3 and S6: the layers and the "roads", "simple" and "slots" sources that the Rust crate makes equal Python's, number for
    number (needs the fixture and cargo)."""
    here, data = Path(__file__).parent, Path(__file__).parent / "data"
    if not (data / "golden.json").exists() or shutil.which("cargo") is None:
        return
    exe = here / "rust" / "target" / "release" / "roadstyle-spike"
    subprocess.run(["cargo", "build", "--release", "--offline", "-q"], cwd=here / "rust", check=True)
    out = tmp_path / "made.json"
    subprocess.run([str(exe), *(str(data / f) for f in ("look.json", "layers.json", "roads.parquet")), str(out)], check=True)
    r = subprocess.run([sys.executable, str(here / "compare.py"), str(data / "golden.json"), str(out), "--only", "layers,sources.roads,sources.simple",
                        "--tol", "0"], capture_output=True, text=True)
    assert r.stdout.strip() == "equal", r.stdout
    # S6: the names and arrows (the slots), against the export with arrows and labels on
    r = subprocess.run([sys.executable, str(here / "compare.py"), str(data / "golden_slots.json"), str(out), "--only", "sources.slots", "--tol", "0"],
                       capture_output=True, text=True)
    assert r.stdout.strip() == "equal", r.stdout


def test_rust_layer_builders_equal_golden():
    """Full port step 2, the first builders: the width, offset and sort expressions the Rust crate makes equal the ones in the golden layers
    (spike/data/layers, tools/make_layer_goldens.py): with twin pairs (all) and without (first100)."""
    here, data = Path(__file__).parent, Path(__file__).parent / "data"
    if not (data / "layers" / "all" / "golden.json").exists() or shutil.which("cargo") is None:
        return
    subprocess.run(["cargo", "build", "--release", "--offline", "-q"], cwd=here / "rust", check=True)
    x = json.loads(subprocess.run([str(here / "rust" / "target" / "release" / "roadstyle-spike"), "--exprs", str(data / "look.json")],
                                  capture_output=True, text=True, check=True).stdout)
    a, f = ({lyr["id"]: lyr for lyr in json.loads((data / "layers" / s / "golden.json").read_text())["layers"]} for s in ("all", "first100"))
    assert diff(a["roads-fill"]["paint"]["line-width"], x["fw"], tol=0) == []
    assert diff(a["roads-fill"]["paint"]["line-offset"], x["off_pairs"], tol=0) == []
    assert diff(a["roads-arrows"]["layout"]["symbol-sort-key"], ["*", -1, x["sort"]], tol=0) == []
    assert diff(f["roads-fill"]["paint"]["line-offset"], x["off"], tol=0) == []


def test_compare_pieces_finds_a_real_change(tmp_path):
    """compare_pieces.py forgives the short-road knife edge and one rounding step, and nothing else."""
    g = Path(__file__).parent / "data" / "golden.json"
    if not g.exists():
        return
    run = lambda b: subprocess.run([sys.executable, str(Path(__file__).parent / "compare_pieces.py"), str(g), str(b)], capture_output=True, text=True)   # noqa: E731
    assert run(g).returncode == 0
    s = json.loads(g.read_text())
    fs = s["sources"]["simple"]["data"]["features"]
    piece = next(f for f in fs if f["properties"]["__rs_k"] == 0 and not f["properties"].get("__rs_seam"))
    piece["geometry"]["coordinates"][0][0] += 1e-5                  # 80 cm: not a rounding step
    del fs[next(i for i, f in enumerate(fs) if f["properties"]["__rs_k"] == 1)]     # a fill missing
    (tmp_path / "x.json").write_text(json.dumps(s))
    r = run(tmp_path / "x.json")
    # both changes are in edge 0 (its first casing piece and its fill): one edge, with the missing piece and the moved point
    assert r.returncode == 1 and "1 otherwise" in r.stdout and ".length: 10 != 9" in r.stdout, r.stdout


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
