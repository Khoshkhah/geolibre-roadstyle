// roadstyle's popup and hover tooltip text: the page (inlined) and the GeoLibre plugin (export_style copies this file, the plugin imports it)
export const POPUP = {maxWidth: "260px", tipOffset: 10};   // a popup's widest, and the hover tooltip's distance from the mouse (px)
export function _rpairs(p, only){  // shared field filter: [nameOrNull, [[k, v], ...]]
  let name=null;
  if(p.name!=null && (""+p.name)!=="" && (""+p.name).toLowerCase()!=="nan") name=""+p.name;
  const ks = (only && only.length) ? only : Object.keys(p), out=[];
  for(const k of ks){ if(k[0]==="_"||k==="lvl"||k==="name") continue;  // lvl is a roadstyle-injected internal (__rs_* are already skipped above)
    var v=p[k]; if(v==null) continue; var s=""+v, sl=s.toLowerCase();
    if(s===""||sl==="nan"||sl==="none") continue;
    if((k==="bridge"||k==="tunnel") && (sl==="no"||sl==="0"||sl==="false")) continue;  // bridge/tunnel only when it IS one
    out.push([k, s]); }
  return [name, out]; }
export function _rfields(p, only){ const [name, kv]=_rpairs(p, only);
  // the field NAME in bold, its value plain - the popup and the hover tooltip both read this way
  return (name ? ["<b>"+name+"</b>"] : []).concat(kv.map(([k,s])=>"<b>"+k+"</b>: "+s)).join("<br>"); }
