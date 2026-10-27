// Generated original device getters: af2952a4; source SHA256 af456401a96e7cab2fc0ad1334e1014ed58d5c694908d2d73ec1ef426463f787.
(context,requestedKey) => {
 const read=name=>{try{return window[name];}catch(_){}};
 const x=window,w=document,u=context?.navigator ?? window.navigator;
 const Da=read("screen");
 const Aa=read("Date");
 const pa=read("localStorage");
 const qc=read("doNotTrack");
 const md=read("sessionStorage");
 const nd=read("devicePixelRatio");
 const od=read("indexedDB");
 const pd=read("openDatabase");
 function ic() {
          var e = u["userAgent"].toLowerCase();
          return e.indexOf("windows phone") >= 0 ? "Windows Phone" : e.indexOf("win") >= 0 && e.indexOf("windvane") < 0 ? "Windows" : e.indexOf("android") >= 0 ? "Android" : e.indexOf("linux") >= 0 ? "Linux" : e.indexOf("iphone") >= 0 || e.indexOf("ipad") >= 0 ? "iOS" : e.indexOf("mac") >= 0 ? "Mac" : "Other";
        }
 const G=ic(),sd=function () {
            var e = ic();
            return e === "Windows Phone" || e === "Android" || e === "iOS" ? 3 : e === "Linux" || e === "Windows" || e === "Mac" ? 2 : 1;
          }(),O=function () {
            var e = u["userAgent"].toLowerCase();
            return e.indexOf("firefox") >= 0 ? "Firefox" : e.indexOf("opera") >= 0 || e.indexOf("opr") >= 0 ? "Opera" : e.indexOf("chrome") >= 0 ? "Chrome" : e.indexOf("safari") >= 0 ? "Safari" : e.indexOf("trident") >= 0 ? "Internet Explorer" : "Other";
          }();
 const fields={"$b":function () {
                return u["userAgent"] || "";
              },
"zb":function () {
                return u["language"] || "";
              },
"Ta":function () {
                return Da["colorDepth"] || 0;
              },
"bb":function () {
                return nd || 0;
              },
"Vb":function () {
                return Math["floor"](new Aa()["getTimezoneOffset"]() / 60 * -1 + 12);
              },
"Pb":function () {
                return !!md;
              },
"Cb":function () {
                return !!pa;
              },
"tb":function () {
                return !!od;
              },
"Ea":function () {
                var a = w["body"];
                return a && !!a["addBehavior"];
              },
"Fb":function () {
                return !!pd;
              },
"Va":function () {
                return u["cpuClass"] || "";
              },
"Ib":function () {
                return u["platform"] || "";
              },
"eb":function () {
                return u["doNotTrack"] ? u["doNotTrack"] : u.yc ? u.yc : qc ? qc : "unknown";
              },
"Da":function () {
                var e = w["createElement"]("div"),
                  c = "fp_" + new Aa();
                e["innerHTML"] = "&nbsp;";
                e.className = "adsbox";
                e.id = c;
                var d = !1;
                try {
                  w["body"]["appendChild"](e), d = w.getElementById(c)["offsetHeight"] === 0, w["body"]["removeChild"](e);
                } catch (f) {
                  d = !1;
                }
                return d;
              },
"pb":function () {
                var e = u["platform"],
                  c = u["oscpu"];
                return ("ontouchstart" in x || u.ua > 0 || u.va > 0) && G !== "Windows Phone" && G !== "Android" && G !== "iOs" && G !== "Other" || typeof c !== "undefined" && (c = c.toLowerCase(), ~c.indexOf("win") && G !== "Windows" && G !== "iOS" && G !== "Other" || ~c.indexOf("linux") && G !== "Linux" && G !== "Android" || ~c.indexOf("mac") && G !== "Mac" && G !== "iOS" || (c.indexOf("win") === -1 && c.indexOf("linux") === -1 && c.indexOf("mac" === -1)) !== (G === "Other")) ? !0 : e.indexOf("win") >= 0 && G !== "Windows" && G !== "Windows Phone" || (e.indexOf("linux") >= 0 || e.indexOf("android") >= 0 || e.indexOf("pike") >= 0) && G !== "Linux" && G !== "Android" || (e.indexOf("mac") >= 0 || e.indexOf("ipad") >= 0 || e.indexOf("ipod") >= 0 || e.indexOf("iphone") >= 0) && G !== "Mac" && G !== "iOS" || (e.indexOf("win") === -1 && e.indexOf("linux") === -1 && e.indexOf("mac") === -1) !== (G === "Other") ? !0 : typeof u["plugins"] === "undefined" && G !== "Windows" && G !== "Windows Phone" ? !0 : !1;
              },
"ob":function () {
                var e = u["productSub"];
                if ((O === "Chrome" || O === "Safari" || O === "Opera") && e !== "20030107") return !0;
                e = eval.toString().length;
                if (e === 37 && O !== "Safari" && O !== "Firefox" && O !== "Other" || e === 39 && O !== "Internet Explorer" && O !== "Other" || e === 33 && O !== "Chrome" && O !== "Opera" && O !== "Other") return !0;
                var c;
                try {
                  throw Error("a");
                } catch (d) {
                  try {
                    d["toSource"](), c = !0;
                  } catch (f) {
                    c = !1;
                  }
                }
                return c && O !== "Firefox" && O !== "Other" ? !0 : !1;
              },
"Yb":function () {
                var e = 0,
                  c = !1;
                typeof u.ua !== "undefined" ? e = u.ua : typeof u.va !== "undefined" && (e = u.va);
                try {
                  w["createEvent"]("TouchEvent"), c = !0;
                } catch (d) {}
                var f = "ontouchstart" in x;
                return e > 0 || c || f;
              },
"Mb":function () {
                return sd;
              },
"Ua":function () {
                return !!u["cookieEnabled"];
              },
"xb":function () {
                try {
                  return !!u["javaEnabled"]();
                } catch (a) {
                  return !1;
                }
              },
"Fa":function () {
                return u["appCodeName"] || "";
              },
"Ga":function () {
                return u["appMinorVersion"] || "";
              },
"Ha":function () {
                return u["appName"] || "";
              },
"Ia":function () {
                return u["appVersion"] || "";
              },
"Ab":function () {
                return u["languages"] || "";
              },
"Sb":function () {
                return u["systemLanguage"] || "";
              },
"ac":function () {
                return u["userLanguage"] || "";
              },
"Na":function () {
                return u["browserLanguage"] || "";
              },
"Gb":function () {
                return u["oscpu"] || "";
              },
"fb":function () {
                return w["documentMode"] || w["compatMode"] || "";
              },
"mb":function () {
                return u["hardwareConcurrency"] || 0;
              },
"Nb":function () {
                var e = Da["width"];
                void 0 === e && (e = 0);
                var c = Da["height"];
                void 0 === c && (c = 0);
                var d = Da["availWidth"];
                void 0 === d && (d = 0);
                var f = Da["availHeight"];
                return [e > c ? e : c, e > c ? c : e, d > f ? d : f, d > f ? f : d];
              }};
 return Object.entries(fields).filter(([key])=>requestedKey===undefined||key===requestedKey).map(([key,f])=>{try{return {key,value:f(),failed:false};}catch(_){return {key,value:null,failed:true};}});
}
