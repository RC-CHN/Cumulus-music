// Generated measurement only: af2952a4 jb; source SHA256 af456401a96e7cab2fc0ad1334e1014ed58d5c694908d2d73ec1ef426463f787.
(context) => {
  try {
    const navigator=context?.navigator ?? window.navigator;
    const read = name => { try { return window[name]; } catch (_) {} };
    const x=window,w=document,u=navigator; let mb,tb,Ba,gc;
    const qd=read('WebGLRenderingContext'),fd=read('ActiveXObject');
    let hashed=false;
    function xa(input) {
      if (typeof input !== 'string') throw new TypeError('fingerprint input must be text');
      hashed=true; return input;
    }
function fc() {
          function e(e) {
            for (var c = !1, l = 0; l < d.length && !(c = e[l]["offsetWidth"] !== m[d[l]] || e[l]["offsetHeight"] !== p[d[l]]); l++);
            return c;
          }
          function c() {
            var a = w["createElement"]("span");
            a["style"]["position"] = "absolute";
            a["style"]["left"] = "-9999px";
            a["style"]["fontSize"] = f;
            a["style"]["lineHeight"] = "normal";
            a["innerHTML"] = l;
            return a;
          }
          if (mb) return mb;
          var d = ["monospace", "sans-serif", "serif"],
            l = "mmmmmmmmmmlli",
            f = "72px",
            h = w["getElementsByTagName"]("body")[0],
            n = w["createElement"]("div"),
            k = w["createElement"]("div"),
            m = {},
            p = {},
            r = function () {
              for (var e = [], l = 0, f = d.length; l < f; l++) {
                var q = c();
                q["style"]["fontFamily"] = d[l];
                n["appendChild"](q);
                e.push(q);
              }
              return e;
            }();
          h["appendChild"](n);
          for (var t = 0, u = d.length; t < u; t++) m[d[t]] = r[t]["offsetWidth"], p[d[t]] = r[t]["offsetHeight"];
          r = function () {
            for (var e = {}, l = 0, f = fontList.length; l < f; l++) {
              for (var q = [], n = 0, h = d.length; n < h; n++) {
                var m;
                m = fontList[l];
                var p = d[n],
                  r = c();
                r["style"]["fontFamily"] = "'" + m + "'," + p;
                m = r;
                k["appendChild"](m);
                q.push(m);
              }
              e[fontList[l]] = q;
            }
            return e;
          }();
          h["appendChild"](k);
          for (var t = [], u = 0, x = fontList.length; u < x; u++) e(r[fontList[u]]) && t.push(fontList[u]);
          h["removeChild"](k);
          h["removeChild"](n);
          return mb = t;
        }
    const value=(function () {
                return xa(fc().join("~"));
              })();
    if (hashed) return {kind:'fingerprint',preimage:value};
    if (value === '') return {kind:'unsupported'};
    throw new TypeError('unexpected measurement result');
  } catch (_) { return {kind:'failed'}; }
}
