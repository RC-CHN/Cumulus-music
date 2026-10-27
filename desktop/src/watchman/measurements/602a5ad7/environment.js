// Generated official browser getters 602a5ad7; SHA256 343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5.
(context) => {
 const y=window,x=document,w=context?.navigator??window.navigator,Ca=Date,cb=window.XMLHttpRequest;let hc,ob;const ic=window.OfflineAudioContext||window.webkitOfflineAudioContext,ya=window.setTimeout.bind(window);const pb=5;
 const C={state:{$:[0,0,0,0,0,0],options:{Ic:0}},h:(_,reading)=>{C.state.$[reading.cursor]=reading.value||0;}};
 const Sa=/./;try{Sa.toString=function(){return Sa.Cc=true;};console.log('%c',Sa);}catch(_){}
 function qb(b, e) {
          for (var d = e.split("."), g = b, h = 0; h < d.length; h++) {
            if (void 0 == g[d[h]]) return;
            g = g[d[h]];
          }
          return g;
        }
function id() {
          for (var c = ["_Selenium_IDE_Recorder", "_phantom", "phantom.injectJs", "callPhantom", "_selenium", "callSelenium", "domAutomation", "domAutomationController", "__nightmare", "domAutomationController", "context.hashCode", "java.lang.System.exit", "spawn", "Buffer", "emit", "webdriver"], e = ["__driver_evaluate", "__webdriver_evaluate", "__selenium_evaluate", "__fxdriver_evaluate", "__driver_unwrapped", "__webdriver_unwrapped", "__selenium_unwrapped", "__fxdriver_unwrapped", "__webdriver_script_function", "__webdriver_script_func", "__webdriver_script_fn"], d = ["selenium", "webdriver", "driver"], h = 0, p = c.length; h < p; h++) if (qb(y, c[h])) return h + 1;
          c = 0;
          for (h = e.length; c < h; c++) if (qb(x, e[c])) return c + 50;
          e = 0;
          for (c = d.length; e < c; e++) if (x["documentElement"]["getAttribute"](d[e])) return e + 100;
          return !0 === qb(w, "webdriver") ? 130 : 0;
        }
function Oa(b) {
          return x["getElementsByTagName"](b) && x["getElementsByTagName"](b).length || 0;
        }
function db() {
          var a;
          try {
            a = new cb();
          } catch (e) {}
          return !!a && "withCredentials" in a;
        }
function ka(a, b, d) {
          return function () {
            var f, g, h;
            d = d || this;
            g = K();
            f = a.apply(d, arguments);
            h = K();
            C.h(pb, {
              cursor: b,
              value: h - g
            });
            return f;
          };
        }
function K() {
          return new Ca()["getTime"]();
        }
function dd(c) {
          function e(a) {
            d(a);
            d = function () {};
          }
          function d(a) {
            return c(a);
          }
          if (hc) return c(hc);
          try {
            var h = new ic(1, 44100, 44100),
              p = h["createOscillator"]();
            p["type"] = "triangle";
            p["frequency"]["value"] = 10000;
            var k = h["createDynamicsCompressor"]();
            k["threshold"] && (k["threshold"]["value"] = -50);
            k["knee"] && (k["knee"]["value"] = 40);
            k["ratio"] && (k["ratio"]["value"] = 12);
            k["reduction"] && (k["reduction"]["value"] = -20);
            k["attack"] && (k["attack"]["value"] = 0);
            k["release"] && (k["release"]["value"] = 0.25);
            p["connect"](k);
            k["connect"](h["destination"]);
            p["start"](0);
            ya(function () {
              e("");
              h["oncomplete"] = function () {};
              h = null;
            }, 1000);
            h["oncomplete"] = function (c) {
              try {
                var d = Xa(za(c["renderedBuffer"]["getChannelData"](0).slice(4500, 5000), function (a, b) {
                  return a + Math.abs(b);
                }).toString());
                e(d);
                p["disconnect"]();
                k["disconnect"]();
              } catch (g) {
                e("");
              }
            };
            h["startRendering"]();
          } catch (m) {
            e("");
          }
        }
function za(c, e) {
          c = c || "~";
          e = e || 0;
          for (var d = c.length % 16, f = c.length - d, p = [0, e], E = [0, e], m = [0, 0], v = [0, 0], l = [2277735313, 289559509], k = [1291169091, 658871167], h = 0; h < f; h += 16) m = [c.charCodeAt(h + 4) & 255 | (c.charCodeAt(h + 5) & 255) << 8 | (c.charCodeAt(h + 6) & 255) << 16 | (c.charCodeAt(h + 7) & 255) << 24, c.charCodeAt(h) & 255 | (c.charCodeAt(h + 1) & 255) << 8 | (c.charCodeAt(h + 2) & 255) << 16 | (c.charCodeAt(h + 3) & 255) << 24], v = [c.charCodeAt(h + 12) & 255 | (c.charCodeAt(h + 13) & 255) << 8 | (c.charCodeAt(h + 14) & 255) << 16 | (c.charCodeAt(h + 15) & 255) << 24, c.charCodeAt(h + 8) & 255 | (c.charCodeAt(h + 9) & 255) << 8 | (c.charCodeAt(h + 10) & 255) << 16 | (c.charCodeAt(h + 11) & 255) << 24], m = R(m, l), m = sa(m, 31), m = R(m, k), p = D(p, m), p = sa(p, 27), p = ea(p, E), p = ea(R(p, [0, 5]), [0, 1390208809]), v = R(v, k), v = sa(v, 33), v = R(v, l), E = D(E, v), E = sa(E, 31), E = ea(E, p), E = ea(R(E, [0, 5]), [0, 944331445]);
          m = [0, 0];
          v = [0, 0];
          switch (d) {
            case 15:
              v = D(v, P([0, c.charCodeAt(h + 14)], 48));
            case 14:
              v = D(v, P([0, c.charCodeAt(h + 13)], 40));
            case 13:
              v = D(v, P([0, c.charCodeAt(h + 12)], 32));
            case 12:
              v = D(v, P([0, c.charCodeAt(h + 11)], 24));
            case 11:
              v = D(v, P([0, c.charCodeAt(h + 10)], 16));
            case 10:
              v = D(v, P([0, c.charCodeAt(h + 9)], 8));
            case 9:
              v = D(v, [0, c.charCodeAt(h + 8)]), v = R(v, k), v = sa(v, 33), v = R(v, l), E = D(E, v);
            case 8:
              m = D(m, P([0, c.charCodeAt(h + 7)], 56));
            case 7:
              m = D(m, P([0, c.charCodeAt(h + 6)], 48));
            case 6:
              m = D(m, P([0, c.charCodeAt(h + 5)], 40));
            case 5:
              m = D(m, P([0, c.charCodeAt(h + 4)], 32));
            case 4:
              m = D(m, P([0, c.charCodeAt(h + 3)], 24));
            case 3:
              m = D(m, P([0, c.charCodeAt(h + 2)], 16));
            case 2:
              m = D(m, P([0, c.charCodeAt(h + 1)], 8));
            case 1:
              m = D(m, [0, c.charCodeAt(h)]), m = R(m, l), m = sa(m, 31), m = R(m, k), p = D(p, m);
          }
          p = D(p, [0, c.length]);
          E = D(E, [0, c.length]);
          p = ea(p, E);
          E = ea(E, p);
          p = Hb(p);
          E = Hb(E);
          p = ea(p, E);
          E = ea(E, p);
          return ("00000000" + (p[0] >>> 0).toString(16)).slice(-8) + ("00000000" + (p[1] >>> 0).toString(16)).slice(-8) + ("00000000" + (E[0] >>> 0).toString(16)).slice(-8) + ("00000000" + (E[1] >>> 0).toString(16)).slice(-8);
        }
function ea(c, b) {
          c = [c[0] >>> 16, c[0] & 65535, c[1] >>> 16, c[1] & 65535];
          b = [b[0] >>> 16, b[0] & 65535, b[1] >>> 16, b[1] & 65535];
          var d = [0, 0, 0, 0];
          d[3] += c[3] + b[3];
          d[2] += d[3] >>> 16;
          d[3] &= 65535;
          d[2] += c[2] + b[2];
          d[1] += d[2] >>> 16;
          d[2] &= 65535;
          d[1] += c[1] + b[1];
          d[0] += d[1] >>> 16;
          d[1] &= 65535;
          d[0] += c[0] + b[0];
          d[0] &= 65535;
          return [d[0] << 16 | d[1], d[2] << 16 | d[3]];
        }
function R(c, b) {
          c = [c[0] >>> 16, c[0] & 65535, c[1] >>> 16, c[1] & 65535];
          b = [b[0] >>> 16, b[0] & 65535, b[1] >>> 16, b[1] & 65535];
          var d = [0, 0, 0, 0];
          d[3] += c[3] * b[3];
          d[2] += d[3] >>> 16;
          d[3] &= 65535;
          d[2] += c[2] * b[3];
          d[1] += d[2] >>> 16;
          d[2] &= 65535;
          d[2] += c[3] * b[2];
          d[1] += d[2] >>> 16;
          d[2] &= 65535;
          d[1] += c[1] * b[3];
          d[0] += d[1] >>> 16;
          d[1] &= 65535;
          d[1] += c[2] * b[2];
          d[0] += d[1] >>> 16;
          d[1] &= 65535;
          d[1] += c[3] * b[1];
          d[0] += d[1] >>> 16;
          d[1] &= 65535;
          d[0] += c[0] * b[3] + c[1] * b[2] + c[2] * b[1] + c[3] * b[0];
          d[0] &= 65535;
          return [d[0] << 16 | d[1], d[2] << 16 | d[3]];
        }
function sa(c, b) {
          b %= 64;
          if (b === 32) return [c[1], c[0]];
          if (b < 32) return [c[0] << b | c[1] >>> 32 - b, c[1] << b | c[0] >>> 32 - b];
          b -= 32;
          return [c[1] << b | c[0] >>> 32 - b, c[0] << b | c[1] >>> 32 - b];
        }
function P(b, e) {
          e %= 64;
          return e === 0 ? b : e < 32 ? [b[0] << e | b[1] >>> 32 - e, b[1] << e] : [b[1] << e - 32, 0];
        }
function D(a, b) {
          return [a[0] ^ b[0], a[1] ^ b[1]];
        }
function Hb(b) {
          b = D(b, [0, b[0] >>> 1]);
          b = R(b, [4283543511, 3981806797]);
          b = D(b, [0, b[0] >>> 1]);
          b = R(b, [3301882366, 444984403]);
          return b = D(b, [0, b[0] >>> 1]);
        }
function Xa(c, e) {
          if (null == c) throw new TypeError("reduce called on null or undefined");
          if (typeof e !== "function") throw new TypeError(e + " is not a function");
          var d = c.length >>> 0,
            f = 0,
            p;
          if (arguments.length === 3) p = arguments[2];else {
            for (; f < d && !(f in c);) f++;
            if (f >= d) throw new TypeError("Reduce of empty array with no initial value");
            p = c[f++];
          }
          for (; f < d; f++) f in c && (p = e(p, c[f], f, c));
          return p;
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
 const fields={"nb":function () {
                return !!Sa.Cc;
              },
"hb":function () {
                var a;
                try {
                  null[0]();
                } catch (e) {
                  a = e;
                }
                return a && typeof a["stack"] === "string" ? ["phantomjs", "rhino", "nodejs", "couchjs", "selenium"].filter(function (b) {
                  return ~a["stack"].indexOf(b);
                })[0] || "" : "";
              },
"Eb":function () {
                for (var c = ["document", "navigator", "location", "history", "", "", "screen", "parent", "top", "self", {
                    q: "parseFloat",
                    n: function () {
                      try {
                        return y["parseFloat"]("1.01") === 1.01 && y["isNaN"](y["parseFloat"]("HI"));
                      } catch (c) {
                        return !1;
                      }
                    }
                  }, {
                    q: "parseInt",
                    n: function () {
                      try {
                        return y["parseInt"]("123") === 123 && y["isNaN"](y["parseFloat"]("HI"));
                      } catch (c) {
                        return !1;
                      }
                    }
                  }, {
                    q: "decodeURI",
                    n: function () {
                      try {
                        return y["decodeURI"]("%22") === "\"";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "decodeURIComponent",
                    n: function () {
                      try {
                        return y["decodeURIComponent"]("%26") === "&";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "encodeURI",
                    n: function () {
                      try {
                        return y["encodeURI"]("\"") === "%22";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "encodeURIComponent",
                    n: function () {
                      try {
                        return y["encodeURIComponent"]("&") === "%26";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "escape",
                    n: function () {
                      try {
                        return y["escape"]("&") === "%26";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "unescape",
                    n: function () {
                      try {
                        return y["unescape"]("%26") === "&";
                      } catch (a) {
                        return !1;
                      }
                    }
                  }, {
                    q: "eval",
                    n: function () {
                      try {
                        return y["eval"]("(function(){return 123;})();") === 123;
                      } catch (c) {
                        return !1;
                      }
                    }
                  }, "window"], e = 0, d = c.length; e < d; e++) {
                  if (c[e].n) if (c[e].n()) continue;else return e + 1;
                  if (c[e] && !y[c[e]]) return e + 1;
                }
                return 0;
              },
"gb":function () {
                var c;
                if (!(c = ka(id, 5, void 0)())) a: {
                  for (var e in x) if (x[e]) {
                    try {
                      if (x[e]["cache_"] && e["match"] && e["match"](/\$[a-z]dc_/)) {
                        c = 200;
                        break a;
                      }
                    } catch (d) {}
                    c = 0;
                    break a;
                  }
                  c = void 0;
                }
                if (!c) try {
                  c = y["external"] && ~y["external"].toString().indexOf("Sequentum") && 201;
                } catch (f) {
                  c = 0;
                }
                return c;
              },
"Xb":function () {
                return y["initWatchman"]["version"] || y["initNEWatchman"]["version"] || 0;
              },
"wb":function () {
                for (var c = !1, e = x["getElementsByTagName"]("script"), d = 0, g = e.length; d < g; d++) {
                  var h = e[d]["src"];
                  if (h && ~h.indexOf("/tool.min.js")) {
                    c = !0;
                    break;
                  }
                }
                return c;
              },
"sb":function () {
                return Oa("iframe");
              },
"kb":function () {
                return Oa("form");
              },
"vb":function () {
                return Oa("input");
              },
"Ob":function () {
                return Oa("script");
              },
"qb":function () {
                return y.history.length || 0;
              },
"Wb":function () {
                return x["title"] || "";
              },
"yb":function () {
                return x.Nc || "";
              },
"Tb":function () {
                return (x["documentElement"]["textContent"] || x["documentElement"]["innerText"]).length || 0;
              },
"rb":function () {
                return x["documentElement"]["innerHTML"].length || 0;
              },
"ub":function () {
                return C["state"].$[0];
              },
"lb":function () {
                return C["state"].$.slice(1);
              },
"Hb":function () {
                var c = y["performance"];
                if (c) {
                  var e = c["timing"];
                  return [c["navigation"].type, e["responseStart"] - e["requestStart"], e["responseEnd"] - e["fetchStart"]];
                }
                return [0, 0, 0];
              },
"Sa":function () {
                return [y["innerWidth"] || x["documentElement"]["clientWidth"] || x["body"]["clientWidth"], y["innerHeight"] || x["documentElement"]["clientHeight"] || x["body"]["clientHeight"]];
              },
"Qb":function () {
                return db() ? 1 : 2;
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
                return gc().length || 0;
              })(),failed:false};}catch(_){return {value:null,failed:true};}},
  audio(){return new Promise(resolve=>{try{(function (a) {
                return ic ? dd(a) : a("");
              })(value=>resolve({value,failed:false}));}catch(_){resolve({value:null,failed:true});}});}
 });
}
