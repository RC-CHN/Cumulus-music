// Generated measurement only: 602a5ad7 Qa; source SHA256 343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5.
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
function lc() {
          var a = x["createElement"]("canvas");
          return !(!a["getContext"] || !a["getContext"]("2d"));
        }
    const value=(function () {
                var c;
                if (lc()) {
                  if (vb) c = vb;else {
                    c = [];
                    try {
                      var e = x["createElement"]("canvas");
                      e["width"] = 500;
                      e["height"] = 200;
                      e["style"].display = "inline";
                      var d = e["getContext"]("2d");
                      d["rect"](0, 0, 10, 10);
                      d["rect"](2, 2, 6, 6);
                      c.push("canvas winding:" + (!1 === d.isPointInPath(5, 5, "evenodd") ? "yes" : "no"));
                      d["textBaseline"] = "alphabetic";
                      d["fillStyle"] = "#f60";
                      d["fillRect"](125, 1, 62, 20);
                      d["fillStyle"] = "#069";
                      d["font"] = "11pt Arial";
                      d.fillText("Cwm fjordbank glyphs vext quiz, 😅😥👶😃🧥🐶🍏⚽️✂🈲🚗⌚️❤️🏁▶", 2, 15);
                      d["fillStyle"] = "rgba(102, 204, 0, 0.2)";
                      d["font"] = "18pt Arial";
                      d.fillText("Cwm fjordbank glyphs vext quiz, 😅😥👶😃", 4, 45);
                      d.fillText("🧥🐶🍏⚽️✂🈲🚗⌚️❤️🏁▶", 4, 75);
                      d["globalCompositeOperation"] = "multiply";
                      d["fillStyle"] = "rgb(255,0,255)";
                      d["beginPath"]();
                      d["arc"](50, 50, 50, 0, Math.PI * 2, !0);
                      d["closePath"]();
                      d.fill();
                      d["fillStyle"] = "rgb(0,255,255)";
                      d["beginPath"]();
                      d["arc"](100, 50, 50, 0, Math.PI * 2, !0);
                      d["closePath"]();
                      d.fill();
                      d["fillStyle"] = "rgb(255,255,0)";
                      d["beginPath"]();
                      d["arc"](75, 100, 50, 0, Math.PI * 2, !0);
                      d["closePath"]();
                      d.fill();
                      d["fillStyle"] = "rgb(255,0,255)";
                      d["arc"](75, 75, 75, 0, Math.PI * 2, !0);
                      d["arc"](75, 75, 25, 0, Math.PI * 2, !0);
                      d.fill("evenodd");
                      c.push("canvas fp:" + e["toDataURL"]());
                    } catch (h) {
                      c.push(h);
                    }
                    c = vb = c;
                  }
                  c = za(c.join("~"));
                } else c = "";
                return c;
              })();
    if (hashed) return {kind:'fingerprint',preimage:value};
    if (value === '') return {kind:'unsupported'};
    throw new TypeError('unexpected measurement result');
  } catch (_) { return {kind:'failed'}; }
}
