// Generated measurement only: 602a5ad7 jb; source SHA256 343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5.
(context) => {
  try {
    const navigator=context?.navigator ?? window.navigator;
    const read = name => { try { return window[name]; } catch (_) {} };
    const y=window,x=document,w=navigator; let ob,vb,Da,hc;
    const qd=read('WebGLRenderingContext'),fd=read('ActiveXObject');
    let hashed=false;
    function za(input) {
      if (typeof input !== 'string') throw new TypeError('fingerprint input must be text');
      hashed=true; return input;
    }
function gc() {
          function c(c) {
            for (var e = !1, f = 0; f < d.length && !(e = c[f]["offsetWidth"] !== q[d[f]] || c[f]["offsetHeight"] !== n[d[f]]); f++);
            return e;
          }
          function e() {
            var a = x["createElement"]("span");
            a["style"]["position"] = "absolute";
            a["style"]["left"] = "-9999px";
            a["style"]["fontSize"] = p;
            a["style"]["lineHeight"] = "normal";
            a["innerHTML"] = h;
            return a;
          }
          if (ob) return ob;
          var d = ["monospace", "sans-serif", "serif"],
            h = "mmmmmmmmmmlli",
            p = "72px",
            k = x["getElementsByTagName"]("body")[0],
            m = x["createElement"]("div"),
            l = x["createElement"]("div"),
            q = {},
            n = {},
            u = function () {
              for (var c = [], g = 0, p = d.length; g < p; g++) {
                var h = e();
                h["style"]["fontFamily"] = d[g];
                m["appendChild"](h);
                c.push(h);
              }
              return c;
            }();
          k["appendChild"](m);
          for (var w = 0, y = d.length; w < y; w++) q[d[w]] = u[w]["offsetWidth"], n[d[w]] = u[w]["offsetHeight"];
          u = function () {
            for (var c = {}, g = 0, p = fontList.length; g < p; g++) {
              for (var h = [], m = 0, k = d.length; m < k; m++) {
                var t;
                t = fontList[g];
                var q = d[m],
                  n = e();
                n["style"]["fontFamily"] = "'" + t + "'," + q;
                t = n;
                l["appendChild"](t);
                h.push(t);
              }
              c[fontList[g]] = h;
            }
            return c;
          }();
          k["appendChild"](l);
          for (var w = [], y = 0, z = fontList.length; y < z; y++) c(u[fontList[y]]) && w.push(fontList[y]);
          k["removeChild"](l);
          k["removeChild"](m);
          return ob = w;
        }
    const value=(function () {
                return za(gc().join("~"));
              })();
    if (hashed) return {kind:'fingerprint',preimage:value};
    if (value === '') return {kind:'unsupported'};
    throw new TypeError('unexpected measurement result');
  } catch (_) { return {kind:'failed'}; }
}
