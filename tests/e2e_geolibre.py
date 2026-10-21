"""The plugin in the real GeoLibre (https://web.geolibre.app), in a headless browser: install the .zip, activate the plugin, open a
database through the plugin's dialog, and switch the "residential" road type off. Needs the network, playwright and a few minutes.

    python build.py --zip
    python tests/e2e_geolibre.py /path/to/monaco.duckdb [screenshots-folder]
"""
import io
import sys
from pathlib import Path

from PIL import Image, ImageChops
from playwright.sync_api import sync_playwright

ROOT = Path(__file__).resolve().parent.parent
ZIP = ROOT / "dist" / "roadstyle-geolibre.zip"


def changed(a, b):
    d = ImageChops.difference(Image.open(io.BytesIO(a)).convert("RGB"), Image.open(io.BytesIO(b)).convert("RGB"))
    return sum(1 for v in d.getdata() if sum(v) > 40)


def main(db, out=None):
    with sync_playwright() as p:
        browser = p.chromium.launch(args=["--use-gl=swiftshader", "--enable-unsafe-swiftshader"])
        pg = browser.new_page(viewport={"width": 1500, "height": 900})
        errors, said = [], []
        pg.on("pageerror", lambda e: errors.append(str(e)[:200]))
        pg.on("console", lambda m: said.append(m.text) if "roadstyle" in m.text.lower() else None)
        pg.goto("https://web.geolibre.app/", wait_until="load")
        pg.wait_for_timeout(15000)
        pg.get_by_role("button", name="Settings").first.click()
        pg.get_by_text("Manage Plugins", exact=True).first.click()
        pg.locator("[role=dialog] button", has_text="Settings").last.click()
        with pg.expect_file_chooser() as fc:
            pg.get_by_role("button", name="Choose .zip…").click()
        fc.value.set_files(str(ZIP))
        pg.get_by_text('Installed plugin "roadstyle"').wait_for(timeout=20000)
        pg.locator("[role=dialog] button", has_text="Close").last.click()
        pg.get_by_role("button", name="Plugins").first.click()
        pg.get_by_text("Installed", exact=True).first.click()
        pg.get_by_text("roadstyle", exact=True).first.click()            # the plugin is installed, not active, until this
        pg.get_by_role("button", name="roadstyle").first.click()
        pg.get_by_text("Open a duckOSM database", exact=False).first.click()
        pg.wait_for_selector("[role=dialog] input[type=file]")
        pg.set_input_files("[role=dialog] input[type=file]", str(db))
        pg.click("[role=dialog] button[type=submit]")
        pg.wait_for_selector(".rs-pl-progress", timeout=20000)
        pg.wait_for_selector(".rs-pl-progress", state="detached", timeout=400000)
        pg.wait_for_timeout(8000)
        clip = {"x": 470, "y": 100, "width": 800, "height": 600}
        before = pg.screenshot(clip=clip)
        row = pg.get_by_text("residential", exact=True).first
        row.scroll_into_view_if_needed()
        box = row.bounding_box()
        pg.mouse.click(box["x"] - 29, box["y"] + box["height"] / 2)      # the eye of the row
        pg.wait_for_timeout(3000)
        after = pg.screenshot(clip=clip)
        if out:
            Path(out).mkdir(parents=True, exist_ok=True)
            (Path(out) / "before.png").write_bytes(before)
            (Path(out) / "after.png").write_bytes(after)
        n = changed(before, after)
        print("pixels changed when the residential roads are switched off:", n)
        assert n > 1000, "the road type row did not change the map"
        assert any("road type residential hidden" in t for t in said), said[-5:]
        assert not errors, errors
        browser.close()
        print("ok")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else None)
