// A probe for tests/e2e_geolibre.py: it puts the map and the app of GeoLibre on window.__rs, so the test can look at the map.
export const plugin = {id: "rs-probe", name: "rs-probe", version: "0.0.1", activate(app) { window.__rs = {app, map: app.getMap()}; }, deactivate() {}};
export default plugin;
