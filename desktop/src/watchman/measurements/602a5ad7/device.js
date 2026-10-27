// Generated original device getters: 602a5ad7; source SHA256 343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5.
(context,requestedKey) => {
 const read=name=>{try{return window[name];}catch(_){}};
 const y=window,x=document,w=context?.navigator ?? window.navigator;
 const Fa=read("screen");
 const Ca=read("Date");
 const ra=read("localStorage");
 const rc=read("doNotTrack");
 const md=read("sessionStorage");
 const nd=read("devicePixelRatio");
 const od=read("indexedDB");
 const pd=read("openDatabase");
 function jc() {
          var c = w["userAgent"].toLowerCase();
          return c.indexOf("windows phone") >= 0 ? "Windows Phone" : c.indexOf("win") >= 0 && c.indexOf("windvane") < 0 ? "Windows" : c.indexOf("android") >= 0 ? "Android" : c.indexOf("linux") >= 0 ? "Linux" : c.indexOf("iphone") >= 0 || c.indexOf("ipad") >= 0 ? "iOS" : c.indexOf("mac") >= 0 ? "Mac" : "Other";
        }
 const H=jc(),sd=function () {
            var c = jc();
            return c === "Windows Phone" || c === "Android" || c === "iOS" ? 3 : c === "Linux" || c === "Windows" || c === "Mac" ? 2 : 1;
          }(),Q=function () {
            var c = w["userAgent"].toLowerCase();
            return c.indexOf("firefox") >= 0 ? "Firefox" : c.indexOf("opera") >= 0 || c.indexOf("opr") >= 0 ? "Opera" : c.indexOf("chrome") >= 0 ? "Chrome" : c.indexOf("safari") >= 0 ? "Safari" : c.indexOf("trident") >= 0 ? "Internet Explorer" : "Other";
          }();
 const fields={"$b":function () {
                return w["userAgent"] || "";
              },
"zb":function () {
                return w["language"] || "";
              },
"Ta":function () {
                return Fa["colorDepth"] || 0;
              },
"bb":function () {
                return nd || 0;
              },
"Vb":function () {
                return Math["floor"](new Ca()["getTimezoneOffset"]() / 60 * -1 + 12);
              },
"Pb":function () {
                return !!md;
              },
"Cb":function () {
                return !!ra;
              },
"tb":function () {
                return !!od;
              },
"Ea":function () {
                var a = x["body"];
                return a && !!a["addBehavior"];
              },
"Fb":function () {
                return !!pd;
              },
"Va":function () {
                return w["cpuClass"] || "";
              },
"Ib":function () {
                return w["platform"] || "";
              },
"eb":function () {
                return w["doNotTrack"] ? w["doNotTrack"] : w.yc ? w.yc : rc ? rc : "unknown";
              },
"Da":function () {
                var c = x["createElement"]("div"),
                  e = "fp_" + new Ca();
                c["innerHTML"] = "&nbsp;";
                c.className = "adsbox";
                c.id = e;
                var d = !1;
                try {
                  x["body"]["appendChild"](c), d = x.getElementById(e)["offsetHeight"] === 0, x["body"]["removeChild"](c);
                } catch (h) {
                  d = !1;
                }
                return d;
              },
"pb":function () {
                var c = w["platform"],
                  e = w["oscpu"];
                return ("ontouchstart" in y || w.ua > 0 || w.va > 0) && H !== "Windows Phone" && H !== "Android" && H !== "iOs" && H !== "Other" || typeof e !== "undefined" && (e = e.toLowerCase(), ~e.indexOf("win") && H !== "Windows" && H !== "iOS" && H !== "Other" || ~e.indexOf("linux") && H !== "Linux" && H !== "Android" || ~e.indexOf("mac") && H !== "Mac" && H !== "iOS" || (e.indexOf("win") === -1 && e.indexOf("linux") === -1 && e.indexOf("mac" === -1)) !== (H === "Other")) ? !0 : c.indexOf("win") >= 0 && H !== "Windows" && H !== "Windows Phone" || (c.indexOf("linux") >= 0 || c.indexOf("android") >= 0 || c.indexOf("pike") >= 0) && H !== "Linux" && H !== "Android" || (c.indexOf("mac") >= 0 || c.indexOf("ipad") >= 0 || c.indexOf("ipod") >= 0 || c.indexOf("iphone") >= 0) && H !== "Mac" && H !== "iOS" || (c.indexOf("win") === -1 && c.indexOf("linux") === -1 && c.indexOf("mac") === -1) !== (H === "Other") ? !0 : typeof w["plugins"] === "undefined" && H !== "Windows" && H !== "Windows Phone" ? !0 : !1;
              },
"ob":function () {
                var c = w["productSub"];
                if ((Q === "Chrome" || Q === "Safari" || Q === "Opera") && c !== "20030107") return !0;
                c = eval.toString().length;
                if (c === 37 && Q !== "Safari" && Q !== "Firefox" && Q !== "Other" || c === 39 && Q !== "Internet Explorer" && Q !== "Other" || c === 33 && Q !== "Chrome" && Q !== "Opera" && Q !== "Other") return !0;
                var e;
                try {
                  throw Error("a");
                } catch (d) {
                  try {
                    d["toSource"](), e = !0;
                  } catch (h) {
                    e = !1;
                  }
                }
                return e && Q !== "Firefox" && Q !== "Other" ? !0 : !1;
              },
"Yb":function () {
                var c = 0,
                  e = !1;
                typeof w.ua !== "undefined" ? c = w.ua : typeof w.va !== "undefined" && (c = w.va);
                try {
                  x["createEvent"]("TouchEvent"), e = !0;
                } catch (d) {}
                var f = "ontouchstart" in y;
                return c > 0 || e || f;
              },
"Mb":function () {
                return sd;
              },
"Ua":function () {
                return !!w["cookieEnabled"];
              },
"xb":function () {
                try {
                  return !!w["javaEnabled"]();
                } catch (a) {
                  return !1;
                }
              },
"Fa":function () {
                return w["appCodeName"] || "";
              },
"Ga":function () {
                return w["appMinorVersion"] || "";
              },
"Ha":function () {
                return w["appName"] || "";
              },
"Ia":function () {
                return w["appVersion"] || "";
              },
"Ab":function () {
                return w["languages"] || "";
              },
"Sb":function () {
                return w["systemLanguage"] || "";
              },
"ac":function () {
                return w["userLanguage"] || "";
              },
"Na":function () {
                return w["browserLanguage"] || "";
              },
"Gb":function () {
                return w["oscpu"] || "";
              },
"fb":function () {
                return x["documentMode"] || x["compatMode"] || "";
              },
"mb":function () {
                return w["hardwareConcurrency"] || 0;
              },
"Nb":function () {
                var c = Fa["width"];
                void 0 === c && (c = 0);
                var e = Fa["height"];
                void 0 === e && (e = 0);
                var d = Fa["availWidth"];
                void 0 === d && (d = 0);
                var f = Fa["availHeight"];
                return [c > e ? c : e, c > e ? e : c, d > f ? d : f, d > f ? f : d];
              }};
 return Object.entries(fields).filter(([key])=>requestedKey===undefined||key===requestedKey).map(([key,f])=>{try{return {key,value:f(),failed:false};}catch(_){return {key,value:null,failed:true};}});
}
