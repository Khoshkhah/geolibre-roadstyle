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
    subprocess.run([str(exe), str(data / "look.json"), str(data / "roads.parquet"), str(out)], check=True)
    r = subprocess.run([sys.executable, str(here / "compare.py"), str(data / "golden.json"), str(out), "--only", "sources.roads,sources.simple",
                        "--tol", "0"], capture_output=True, text=True)
    assert r.stdout.strip() == "equal", r.stdout
    # step 2: the road layers, made in Rust, against the golden of all the roads with arrows and labels
    r = subprocess.run([sys.executable, str(here / "compare.py"), str(data / "layers" / "all" / "golden.json"), str(out), "--only", "layers",
                        "--tol", "0"], capture_output=True, text=True)
    assert r.stdout.strip() == "equal", r.stdout
    # S6: the names and arrows (the slots), against the export with arrows and labels on
    r = subprocess.run([sys.executable, str(here / "compare.py"), str(data / "golden_slots.json"), str(out), "--only", "sources.slots", "--tol", "0"],
                       capture_output=True, text=True)
    assert r.stdout.strip() == "equal", r.stdout


def test_rust_layers_equal_golden_for_every_subset():
    """Full port step 2: the road layers the Rust crate makes for each of the 12 subsets (spike/data/layers, tools/make_layer_goldens.py: tunnels,
    twin pairs, reverse pairs, dash values, no one-way slot, no name) equal their golden layers with tolerance 0."""
    here, data = Path(__file__).parent, Path(__file__).parent / "data" / "layers"
    if not data.exists() or shutil.which("cargo") is None:
        return
    subprocess.run(["cargo", "build", "--release", "--offline", "-q"], cwd=here / "rust", check=True)
    exe = here / "rust" / "target" / "release" / "roadstyle-spike"
    subsets = sorted(p for p in data.iterdir() if (p / "golden.json").exists())
    assert len(subsets) == 12
    for d in subsets:
        made = json.loads(subprocess.run([str(exe), "--layers", str(here / "data" / "look.json"), str(d / "roads.parquet")],
                                         capture_output=True, text=True, check=True).stdout)
        assert diff(json.loads((d / "golden.json").read_text()), made, tol=0) == [], d.name


def test_rust_base_map_sources_equal_golden():
    """Full port step 3, part 1: the base-map feature collections the Rust crate makes from features.parquet (the worker's SQL) equal the
    golden style's ov<i> sources: the properties with tolerance 0, the coordinates within 1e-12 (a centroid summed in another order). The street
    of a crossing or a signal (edge_id, __rs_*) comes in part 2 and is left out here."""
    here, data = Path(__file__).parent, Path(__file__).parent / "data"
    if not (data / "features.parquet").exists() or shutil.which("cargo") is None:
        return
    subprocess.run(["cargo", "build", "--release", "--offline", "-q"], cwd=here / "rust", check=True)
    fcs = json.loads(subprocess.run([str(here / "rust" / "target" / "release" / "roadstyle-spike"), "--features", str(data / "look.json"),
                                     str(data / "features.parquet")], capture_output=True, text=True, check=True).stdout)
    golden = json.loads((data / "golden_full.json").read_text())["sources"]
    strip = lambda fs: [{"properties": {k: v for k, v in f["properties"].items() if k != "edge_id" and not k.startswith("__rs_")},  # noqa: E731
                         "geometry": f["geometry"]} for f in fs]
    assert len(fcs) == len([k for k in golden if k.startswith("ov")]) == 14
    for i, (name, fc) in enumerate(fcs.items()):
        assert diff(strip(golden[f"ov{i}"]["data"]["features"]), strip(fc["features"]), tol=1e-12) == [], name


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
