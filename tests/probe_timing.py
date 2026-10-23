"""Where does the time go when the plugin opens a duckOSM database? The steps of the progress box, with the seconds since the click on Open.

    python tests/probe_timing.py /path/to/monaco.duckdb [on|off]        # the route planner on or off (default on)
"""
import sys
import tempfile
import time
from pathlib import Path

from playwright.sync_api import sync_playwright

sys.path.insert(0, str(Path(__file__).resolve().parent))
from e2e_geolibre import ZIP, activate, probe_zip  # noqa: E402


def main(db, planner="on"):
    with tempfile.TemporaryDirectory() as tmp, sync_playwright() as p:
        b = p.chromium.launch(args=["--use-gl=swiftshader", "--enable-unsafe-swiftshader"])
        pg = b.new_page(viewport={"width": 1500, "height": 900})
        pg.goto("https://web.geolibre.app/", wait_until="load")
        pg.wait_for_timeout(15000)
        pg.get_by_role("button", name="Settings").first.click()
        pg.get_by_text("Manage Plugins", exact=True).first.click()
        pg.locator("[role=dialog] button", has_text="Settings").last.click()
        for z in (ZIP, probe_zip(tmp)):
            with pg.expect_file_chooser() as fc:
                pg.get_by_role("button", name="Choose .zip…").click()
            fc.value.set_files(str(z))
            pg.wait_for_timeout(3500)
        pg.locator("[role=dialog] button", has_text="Close").last.click()
        pg.wait_for_timeout(800)
        activate(pg, "roadstyle")
        activate(pg, "rs-probe")
        for round_ in (1, 2):                      # the second time: the same file, the same choices (the style comes from the browser's storage)
            print(f"--- opening the file, time {round_}")
            pg.get_by_role("button", name="roadstyle").first.click()
            pg.get_by_text("Open a duckOSM database", exact=False).first.click()
            pg.wait_for_selector("[role=dialog] input[type=file]")
            pg.set_input_files("[role=dialog] input[type=file]", str(db))
            pg.select_option("[role=dialog] select >> nth=4", planner)
            t0 = time.time()
            pg.click("[role=dialog] button[type=submit]")
            pg.wait_for_selector(".rs-pl-progress", timeout=20000)
            last, steps = None, []
            while pg.query_selector(".rs-pl-progress"):
                txt = pg.evaluate("() => (document.querySelector('.rs-pl-progress') || {}).innerText || ''").strip().replace("\n", " | ")[:90]
                if txt != last:
                    steps.append((round(time.time() - t0, 1), txt)); last = txt
                pg.wait_for_timeout(400)
            steps.append((round(time.time() - t0, 1), "progress box gone"))
            # the map is drawn: MapLibre idle
            pg.wait_for_function("() => window.__rs.map.loaded()", timeout=300000)
            steps.append((round(time.time() - t0, 1), "map idle (all data on the screen)"))
            for t, s in steps:
                print(f"{t:7.1f} s  {s}")
            print("planner:", planner, "| total", steps[-1][0], "s")
            pg.get_by_role("button", name="roadstyle").first.click()
            pg.get_by_text("Remove roadstyle layers", exact=False).first.click()
            pg.wait_for_timeout(3000)
        b.close()


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else "on")
