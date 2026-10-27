// Generated measurement only: af2952a4 Qa; source SHA256 af456401a96e7cab2fc0ad1334e1014ed58d5c694908d2d73ec1ef426463f787.
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
function kc() {
          var a = w["createElement"]("canvas");
          return !(!a["getContext"] || !a["getContext"]("2d"));
        }
    const value=(function () {
                var e;
                if (kc()) {
                  if (tb) e = tb;else {
                    e = [];
                    try {
                      var c = w["createElement"]("canvas");
                      c["width"] = 500;
                      c["height"] = 200;
                      c["style"].display = "inline";
                      var d = c["getContext"]("2d");
                      d["rect"](0, 0, 10, 10);
                      d["rect"](2, 2, 6, 6);
                      e.push("canvas winding:" + (!1 === d.isPointInPath(5, 5, "evenodd") ? "yes" : "no"));
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
                      e.push("canvas fp:" + c["toDataURL"]());
                    } catch (f) {
                      e.push(f);
                    }
                    e = tb = e;
                  }
                  e = xa(e.join("~"));
                } else e = "";
                return e;
              })();
    if (hashed) return {kind:'fingerprint',preimage:value};
    if (value === '') return {kind:'unsupported'};
    throw new TypeError('unexpected measurement result');
  } catch (_) { return {kind:'failed'}; }
}
