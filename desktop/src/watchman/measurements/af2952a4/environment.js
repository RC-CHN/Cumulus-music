// Generated official browser getters af2952a4; SHA256 af456401a96e7cab2fc0ad1334e1014ed58d5c694908d2d73ec1ef426463f787.
(context) => {
 const x=window,w=document,u=context?.navigator??window.navigator,Aa=Date,ab=window.XMLHttpRequest;let gc,mb;const hc=window.OfflineAudioContext||window.webkitOfflineAudioContext,wa=window.setTimeout.bind(window);const nb=5;
 const C={state:{$:[0,0,0,0,0,0],options:{Ic:0}},h:(_,reading)=>{C.state.$[reading.cursor]=reading.value||0;}};
 const Qa=/./;try{Qa.toString=function(){return Qa.Cc=true;};console.log('%c',Qa);}catch(_){}
 function ob(e, c) {
          for (var d = c.split("."), l = e, g = 0; g < d.length; g++) {
            if (void 0 == l[d[g]]) return;
            l = l[d[g]];
          }
          return l;
        }
function id() {
          for (var e = ["_Selenium_IDE_Recorder", "_phantom", "phantom.injectJs", "callPhantom", "_selenium", "callSelenium", "domAutomation", "domAutomationController", "__nightmare", "domAutomationController", "context.hashCode", "java.lang.System.exit", "spawn", "Buffer", "emit", "webdriver"], c = ["__driver_evaluate", "__webdriver_evaluate", "__selenium_evaluate", "__fxdriver_evaluate", "__driver_unwrapped", "__webdriver_unwrapped", "__selenium_unwrapped", "__fxdriver_unwrapped", "__webdriver_script_function", "__webdriver_script_func", "__webdriver_script_fn"], d = ["selenium", "webdriver", "driver"], l = 0, f = e.length; l < f; l++) if (ob(x, e[l])) return l + 1;
          e = 0;
          for (l = c.length; e < l; e++) if (ob(w, c[e])) return e + 50;
          c = 0;
          for (e = d.length; c < e; c++) if (w["documentElement"]["getAttribute"](d[c])) return c + 100;
          return !0 === ob(u, "webdriver") ? 130 : 0;
        }
function Ma(e) {
          return w["getElementsByTagName"](e) && w["getElementsByTagName"](e).length || 0;
        }
function bb() {
          var a;
          try {
            a = new ab();
          } catch (b) {}
          return !!a && "withCredentials" in a;
        }
function ia(a, b, d) {
          return function () {
            var l, g, f;
            d = d || this;
            g = I();
            l = a.apply(d, arguments);
            f = I();
            C.h(nb, {
              cursor: b,
              value: f - g
            });
            return l;
          };
        }
function I() {
          return new Aa()["getTime"]();
        }
function dd(e) {
          function c(a) {
            d(a);
            d = function () {};
          }
          function d(a) {
            return e(a);
          }
          if (gc) return e(gc);
          try {
            var l = new hc(1, 44100, 44100),
              f = l["createOscillator"]();
            f["type"] = "triangle";
            f["frequency"]["value"] = 10000;
            var h = l["createDynamicsCompressor"]();
            h["threshold"] && (h["threshold"]["value"] = -50);
            h["knee"] && (h["knee"]["value"] = 40);
            h["ratio"] && (h["ratio"]["value"] = 12);
            h["reduction"] && (h["reduction"]["value"] = -20);
            h["attack"] && (h["attack"]["value"] = 0);
            h["release"] && (h["release"]["value"] = 0.25);
            f["connect"](h);
            h["connect"](l["destination"]);
            f["start"](0);
            wa(function () {
              c("");
              l["oncomplete"] = function () {};
              l = null;
            }, 1000);
            l["oncomplete"] = function (e) {
              try {
                var d = Va(xa(e["renderedBuffer"]["getChannelData"](0).slice(4500, 5000), function (a, b) {
                  return a + Math.abs(b);
                }).toString());
                c(d);
                f["disconnect"]();
                h["disconnect"]();
              } catch (l) {
                c("");
              }
            };
            l["startRendering"]();
          } catch (n) {
            c("");
          }
        }
function xa(e, c) {
          e = e || "~";
          c = c || 0;
          for (var d = e.length % 16, l = e.length - d, q = [0, c], g = [0, c], n = [0, 0], v = [0, 0], m = [2277735313, 289559509], h = [1291169091, 658871167], f = 0; f < l; f += 16) n = [e.charCodeAt(f + 4) & 255 | (e.charCodeAt(f + 5) & 255) << 8 | (e.charCodeAt(f + 6) & 255) << 16 | (e.charCodeAt(f + 7) & 255) << 24, e.charCodeAt(f) & 255 | (e.charCodeAt(f + 1) & 255) << 8 | (e.charCodeAt(f + 2) & 255) << 16 | (e.charCodeAt(f + 3) & 255) << 24], v = [e.charCodeAt(f + 12) & 255 | (e.charCodeAt(f + 13) & 255) << 8 | (e.charCodeAt(f + 14) & 255) << 16 | (e.charCodeAt(f + 15) & 255) << 24, e.charCodeAt(f + 8) & 255 | (e.charCodeAt(f + 9) & 255) << 8 | (e.charCodeAt(f + 10) & 255) << 16 | (e.charCodeAt(f + 11) & 255) << 24], n = P(n, m), n = qa(n, 31), n = P(n, h), q = D(q, n), q = qa(q, 27), q = ca(q, g), q = ca(P(q, [0, 5]), [0, 1390208809]), v = P(v, h), v = qa(v, 33), v = P(v, m), g = D(g, v), g = qa(g, 31), g = ca(g, q), g = ca(P(g, [0, 5]), [0, 944331445]);
          n = [0, 0];
          v = [0, 0];
          switch (d) {
            case 15:
              v = D(v, N([0, e.charCodeAt(f + 14)], 48));
            case 14:
              v = D(v, N([0, e.charCodeAt(f + 13)], 40));
            case 13:
              v = D(v, N([0, e.charCodeAt(f + 12)], 32));
            case 12:
              v = D(v, N([0, e.charCodeAt(f + 11)], 24));
            case 11:
              v = D(v, N([0, e.charCodeAt(f + 10)], 16));
            case 10:
              v = D(v, N([0, e.charCodeAt(f + 9)], 8));
            case 9:
              v = D(v, [0, e.charCodeAt(f + 8)]), v = P(v, h), v = qa(v, 33), v = P(v, m), g = D(g, v);
            case 8:
              n = D(n, N([0, e.charCodeAt(f + 7)], 56));
            case 7:
              n = D(n, N([0, e.charCodeAt(f + 6)], 48));
            case 6:
              n = D(n, N([0, e.charCodeAt(f + 5)], 40));
            case 5:
              n = D(n, N([0, e.charCodeAt(f + 4)], 32));
            case 4:
              n = D(n, N([0, e.charCodeAt(f + 3)], 24));
            case 3:
              n = D(n, N([0, e.charCodeAt(f + 2)], 16));
            case 2:
              n = D(n, N([0, e.charCodeAt(f + 1)], 8));
            case 1:
              n = D(n, [0, e.charCodeAt(f)]), n = P(n, m), n = qa(n, 31), n = P(n, h), q = D(q, n);
          }
          q = D(q, [0, e.length]);
          g = D(g, [0, e.length]);
          q = ca(q, g);
          g = ca(g, q);
          q = Gb(q);
          g = Gb(g);
          q = ca(q, g);
          g = ca(g, q);
          return ("00000000" + (q[0] >>> 0).toString(16)).slice(-8) + ("00000000" + (q[1] >>> 0).toString(16)).slice(-8) + ("00000000" + (g[0] >>> 0).toString(16)).slice(-8) + ("00000000" + (g[1] >>> 0).toString(16)).slice(-8);
        }
function ca(b, c) {
          b = [b[0] >>> 16, b[0] & 65535, b[1] >>> 16, b[1] & 65535];
          c = [c[0] >>> 16, c[0] & 65535, c[1] >>> 16, c[1] & 65535];
          var d = [0, 0, 0, 0];
          d[3] += b[3] + c[3];
          d[2] += d[3] >>> 16;
          d[3] &= 65535;
          d[2] += b[2] + c[2];
          d[1] += d[2] >>> 16;
          d[2] &= 65535;
          d[1] += b[1] + c[1];
          d[0] += d[1] >>> 16;
          d[1] &= 65535;
          d[0] += b[0] + c[0];
          d[0] &= 65535;
          return [d[0] << 16 | d[1], d[2] << 16 | d[3]];
        }
function P(b, c) {
          b = [b[0] >>> 16, b[0] & 65535, b[1] >>> 16, b[1] & 65535];
          c = [c[0] >>> 16, c[0] & 65535, c[1] >>> 16, c[1] & 65535];
          var d = [0, 0, 0, 0];
          d[3] += b[3] * c[3];
          d[2] += d[3] >>> 16;
          d[3] &= 65535;
          d[2] += b[2] * c[3];
          d[1] += d[2] >>> 16;
          d[2] &= 65535;
          d[2] += b[3] * c[2];
          d[1] += d[2] >>> 16;
          d[2] &= 65535;
          d[1] += b[1] * c[3];
          d[0] += d[1] >>> 16;
          d[1] &= 65535;
          d[1] += b[2] * c[2];
          d[0] += d[1] >>> 16;
          d[1] &= 65535;
          d[1] += b[3] * c[1];
          d[0] += d[1] >>> 16;
          d[1] &= 65535;
          d[0] += b[0] * c[3] + b[1] * c[2] + b[2] * c[1] + b[3] * c[0];
          d[0] &= 65535;
          return [d[0] << 16 | d[1], d[2] << 16 | d[3]];
        }
function qa(b, c) {
          c %= 64;
          if (c === 32) return [b[1], b[0]];
          if (c < 32) return [b[0] << c | b[1] >>> 32 - c, b[1] << c | b[0] >>> 32 - c];
          c -= 32;
          return [b[1] << c | b[0] >>> 32 - c, b[0] << c | b[1] >>> 32 - c];
        }
function N(b, c) {
          c %= 64;
          return c === 0 ? b : c < 32 ? [b[0] << c | b[1] >>> 32 - c, b[1] << c] : [b[1] << c - 32, 0];
        }
function D(a, b) {
          return [a[0] ^ b[0], a[1] ^ b[1]];
        }
function Gb(b) {
          b = D(b, [0, b[0] >>> 1]);
          b = P(b, [4283543511, 3981806797]);
          b = D(b, [0, b[0] >>> 1]);
          b = P(b, [3301882366, 444984403]);
          return b = D(b, [0, b[0] >>> 1]);
        }
function Va(e, c) {
          if (null == e) throw new TypeError("reduce called on null or undefined");
          if (typeof c !== "function") throw new TypeError(c + " is not a function");
          var d = e.length >>> 0,
            l = 0,
            q;
          if (arguments.length === 3) q = arguments[2];else {
            for (; l < d && !(l in e);) l++;
            if (l >= d) throw new TypeError("Reduce of empty array with no initial value");
            q = e[l++];
          }
          for (; l < d; l++) l in e && (q = c(q, e[l], l, e));
          return q;
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
 const fields={"nb":function () {
                return !!Qa.Cc;
              },
"hb":function () {
                var a;
                try {
                  null[0]();
                } catch (c) {
                  a = c;
                }
                return a && typeof a["stack"] === "string" ? ["phantomjs", "rhino", "nodejs", "couchjs", "selenium"].filter(function (c) {
                  return ~a["stack"].indexOf(c);
                })[0] || "" : "";
              },
"Eb":function () {
                for (var e = ["document", "navigator", "location", "history", "", "", "screen", "parent", "top", "self", {
                    q: "parseFloat",
                    n: function () {
                      try {
                        return x["parseFloat"]("1.01") === 1.01 && x["isNaN"](x["parseFloat"]("HI"));
                      } catch (c) {
                        return !1;
                      }
                    }
                  }, {
                    q: "parseInt",
                    n: function () {
                      try {
                        return x["parseInt"]("123") === 123 && x["isNaN"](x["parseFloat"]("HI"));
                      } catch (c) {
                        return !1;
                      }
                    }
                  }, {
                    q: "decodeURI",
                    n: function () {
                      try {
                        return x["decodeURI"]("%22") === "\"";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "decodeURIComponent",
                    n: function () {
                      try {
                        return x["decodeURIComponent"]("%26") === "&";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "encodeURI",
                    n: function () {
                      try {
                        return x["encodeURI"]("\"") === "%22";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "encodeURIComponent",
                    n: function () {
                      try {
                        return x["encodeURIComponent"]("&") === "%26";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "escape",
                    n: function () {
                      try {
                        return x["escape"]("&") === "%26";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "unescape",
                    n: function () {
                      try {
                        return x["unescape"]("%26") === "&";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "eval",
                    n: function () {
                      try {
                        return x["eval"]("(function(){return 123;})();") === 123;
                      } catch (c) {
                        return !1;
                      }
                    }
                  }, "window"], c = 0, d = e.length; c < d; c++) {
                  if (e[c].n) if (e[c].n()) continue;else return c + 1;
                  if (e[c] && !x[e[c]]) return c + 1;
                }
                return 0;
              },
"gb":function () {
                var e;
                if (!(e = ia(id, 5, void 0)())) a: {
                  for (var c in w) if (w[c]) {
                    try {
                      if (w[c]["cache_"] && c["match"] && c["match"](/\$[a-z]dc_/)) {
                        e = 200;
                        break a;
                      }
                    } catch (d) {}
                    e = 0;
                    break a;
                  }
                  e = void 0;
                }
                if (!e) try {
                  e = x["external"] && ~x["external"].toString().indexOf("Sequentum") && 201;
                } catch (f) {
                  e = 0;
                }
                return e;
              },
"Xb":function () {
                return x["initWatchman"]["version"] || x["initNEWatchman"]["version"] || 0;
              },
"wb":function () {
                for (var e = !1, c = w["getElementsByTagName"]("script"), d = 0, g = c.length; d < g; d++) {
                  var f = c[d]["src"];
                  if (f && ~f.indexOf("/tool.min.js")) {
                    e = !0;
                    break;
                  }
                }
                return e;
              },
"sb":function () {
                return Ma("iframe");
              },
"kb":function () {
                return Ma("form");
              },
"vb":function () {
                return Ma("input");
              },
"Ob":function () {
                return Ma("script");
              },
"qb":function () {
                return x.history.length || 0;
              },
"Wb":function () {
                return w["title"] || "";
              },
"yb":function () {
                return w.Nc || "";
              },
"Tb":function () {
                return (w["documentElement"]["textContent"] || w["documentElement"]["innerText"]).length || 0;
              },
"rb":function () {
                return w["documentElement"]["innerHTML"].length || 0;
              },
"ub":function () {
                return C["state"].$[0];
              },
"lb":function () {
                return C["state"].$.slice(1);
              },
"Hb":function () {
                var e = x["performance"];
                if (e) {
                  var c = e["timing"];
                  return [e["navigation"].type, c["responseStart"] - c["requestStart"], c["responseEnd"] - c["fetchStart"]];
                }
                return [0, 0, 0];
              },
"Sa":function () {
                return [x["innerWidth"] || w["documentElement"]["clientWidth"] || w["body"]["clientWidth"], x["innerHeight"] || w["documentElement"]["clientHeight"] || w["body"]["clientHeight"]];
              },
"Qb":function () {
                return bb() ? 1 : 2;
              },
"cc":function () {
                return C["state"]["options"].Ic;
              }};
 return Object.freeze({
  environment(key,timing,elapsed){
   if(!Object.prototype.hasOwnProperty.call(fields,key))throw new TypeError('unreviewed environment key');
   C.state.$=timing.slice();C.state.options.Ic=elapsed;
   try{return {value:fields[key](),failed:false,timing:C.state.$};}catch(_){return {value:null,failed:true,timing:C.state.$};}
  },
  fontCount(){try{return {value:(function () {
                return fc().length || 0;
              })(),failed:false};}catch(_){return {value:null,failed:true};}},
  audio(){return new Promise(resolve=>{try{(function (a) {
                return hc ? dd(a) : a("");
              })(value=>resolve({value,failed:false}));}catch(_){resolve({value:null,failed:true});}});}
 });
}
