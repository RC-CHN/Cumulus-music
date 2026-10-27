// Generated measurement only: 602a5ad7 dc; source SHA256 343a187cb023c495a2dffee37f8265d203d97886c43fa6180951fb2de4d283b5.
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
function bd() {
          var a = x["createElement"]("canvas"),
            e = null;
          try {
            e = a["getContext"]("webgl") || a["getContext"]("experimental-webgl");
          } catch (d) {}
          e || (e = null);
          return e;
        }
function cd() {
          function c(c) {
            e["clearColor"](0, 0, 0, 1);
            e.enable(e["DEPTH_TEST"]);
            e["depthFunc"](e["LEQUAL"]);
            e.clear(e["COLOR_BUFFER_BIT"] | e["DEPTH_BUFFER_BIT"]);
            return "[" + c[0] + ", " + c[1] + "]";
          }
          if (Da) return Da;
          var e;
          e = bd();
          if (!e) return Da = [];
          var d = [];
          try {
            var h = "attribute vec2 attrVertexvarying vec2 varyinTexCoordinateuniform vec2 uniformOffsetvoid main(){varyinTexCoordinate=attrVertex+uniformOffsetgl_Position=vec4(attrVertex,0,1)}",
              p = "precision mediump floatvarying vec2 varyinTexCoordinatevoid main() {gl_FragColor=vec4(varyinTexCoordinate,0,1)}",
              k = e["createBuffer"]();
            e["bindBuffer"](e["ARRAY_BUFFER"], k);
            var m = new Float32Array([-0.2, -0.9, 0, 0.4, -0.26, 0, 0, 0.732134444, 0]);
            e.bufferData(e["ARRAY_BUFFER"], m, e["STATIC_DRAW"]);
            k.wc = 3;
            k.Ac = 3;
            var l = e["createProgram"](),
              q = e["createShader"](e["VERTEX_SHADER"]);
            e["shaderSource"](q, h);
            e["compileShader"](q);
            var n = e["createShader"](e["FRAGMENT_SHADER"]);
            e["shaderSource"](n, p);
            e["compileShader"](n);
            e["attachShader"](l, q);
            e["attachShader"](l, n);
            e["linkProgram"](l);
            e["useProgram"](l);
            l.Hc = e["getAttribLocation"](l, "attrVertex");
            l.Bc = e["getUniformLocation"](l, "uniformOffset");
            e["enableVertexAttribArray"](l.Qc);
            e["vertexAttribPointer"](l.Hc, k.wc, e.FLOAT, !1, 0, 0);
            e["uniform2f"](l.Bc, 1, 1);
            e["drawArrays"](e["TRIANGLE_STRIP"], 0, k.Ac);
          } catch (u) {}
          null != e["canvas"] && d.push(e["canvas"]["toDataURL"]());
          d.push("extensions:" + e["getSupportedExtensions"]().join(""));
          d.push("webgl aliased line width range:" + c(e["getParameter"](e["ALIASED_LINE_WIDTH_RANGE"])));
          d.push("webgl aliased point size range:" + c(e["getParameter"](e["ALIASED_POINT_SIZE_RANGE"])));
          d.push("webgl alpha bits:" + e["getParameter"](e["ALPHA_BITS"]));
          d.push("webgl antialiasing:" + (e["getContextAttributes"]().antialias ? "yes" : "no"));
          d.push("webgl blue bits:" + e["getParameter"](e["BLUE_BITS"]));
          d.push("webgl depth bits:" + e["getParameter"](e["DEPTH_BITS"]));
          d.push("webgl green bits:" + e["getParameter"](e["GREEN_BITS"]));
          d.push("webgl max anisotropy:" + function (c) {
            var d,
              e = c["getExtension"]("EXT_texture_filter_anisotropic") || c["getExtension"]("WEBKIT_EXT_texture_filter_anisotropic") || c["getExtension"]("MOZ_EXT_texture_filter_anisotropic");
            return e ? (d = c["getParameter"](e["MAX_TEXTURE_MAX_ANISOTROPY_EXT"]), 0 === d && (d = 2), d) : null;
          }(e));
          d.push("webgl max combined texture image units:" + e["getParameter"](e["MAX_COMBINED_TEXTURE_IMAGE_UNITS"]));
          d.push("webgl max cube map texture size:" + e["getParameter"](e["MAX_CUBE_MAP_TEXTURE_SIZE"]));
          d.push("webgl max fragment uniform vectors:" + e["getParameter"](e["MAX_FRAGMENT_UNIFORM_VECTORS"]));
          d.push("webgl max render buffer size:" + e["getParameter"](e["MAX_RENDERBUFFER_SIZE"]));
          d.push("webgl max texture image units:" + e["getParameter"](e["MAX_TEXTURE_IMAGE_UNITS"]));
          d.push("webgl max texture size:" + e["getParameter"](e["MAX_TEXTURE_SIZE"]));
          d.push("webgl max varying vectors:" + e["getParameter"](e["MAX_VARYING_VECTORS"]));
          d.push("webgl max vertex attribs:" + e["getParameter"](e["MAX_VERTEX_ATTRIBS"]));
          d.push("webgl max vertex texture image units:" + e["getParameter"](e["MAX_VERTEX_TEXTURE_IMAGE_UNITS"]));
          d.push("webgl max vertex uniform vectors:" + e["getParameter"](e["MAX_VERTEX_UNIFORM_VECTORS"]));
          d.push("webgl max viewport dims:" + c(e["getParameter"](e["MAX_VIEWPORT_DIMS"])));
          d.push("webgl red bits:" + e["getParameter"](e["RED_BITS"]));
          d.push("webgl renderer:" + e["getParameter"](e["RENDERER"]));
          d.push("webgl shading language version:" + e["getParameter"](e["SHADING_LANGUAGE_VERSION"]));
          d.push("webgl stencil bits:" + e["getParameter"](e["STENCIL_BITS"]));
          d.push("webgl vendor:" + e["getParameter"](e["VENDOR"]));
          d.push("webgl version:" + e["getParameter"](e["VERSION"]));
          try {
            var w = e["getExtension"]("WEBGL_debug_renderer_info");
            w && (d.push("webgl unmasked vendor:" + e["getParameter"](w.UNMASKED_VENDOR_WEBGL)), d.push("webgl unmasked renderer:" + e["getParameter"](w.UNMASKED_RENDERER_WEBGL)));
          } catch (y) {}
          if (!e["getShaderPrecisionFormat"]) return Da = d;
          d.push("webgl vertex shader high float precision:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["HIGH_FLOAT"])["precision"]);
          d.push("webgl vertex shader high float precision rangeMin:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["HIGH_FLOAT"])["rangeMin"]);
          d.push("webgl vertex shader high float precision rangeMax:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["HIGH_FLOAT"])["rangeMax"]);
          d.push("webgl vertex shader medium float precision:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["MEDIUM_FLOAT"])["precision"]);
          d.push("webgl vertex shader medium float precision rangeMin:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["MEDIUM_FLOAT"])["rangeMin"]);
          d.push("webgl vertex shader medium float precision rangeMax:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["MEDIUM_FLOAT"])["rangeMax"]);
          d.push("webgl vertex shader low float precision:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["LOW_FLOAT"])["precision"]);
          d.push("webgl vertex shader low float precision rangeMin:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["LOW_FLOAT"])["rangeMin"]);
          d.push("webgl vertex shader low float precision rangeMax:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["LOW_FLOAT"])["rangeMax"]);
          d.push("webgl fragment shader high float precision:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["HIGH_FLOAT"])["precision"]);
          d.push("webgl fragment shader high float precision rangeMin:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["HIGH_FLOAT"])["rangeMin"]);
          d.push("webgl fragment shader high float precision rangeMax:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["HIGH_FLOAT"])["rangeMax"]);
          d.push("webgl fragment shader medium float precision:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["MEDIUM_FLOAT"])["precision"]);
          d.push("webgl fragment shader medium float precision rangeMin:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["MEDIUM_FLOAT"])["rangeMin"]);
          d.push("webgl fragment shader medium float precision rangeMax:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["MEDIUM_FLOAT"])["rangeMax"]);
          d.push("webgl fragment shader low float precision:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["LOW_FLOAT"])["precision"]);
          d.push("webgl fragment shader low float precision rangeMin:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["LOW_FLOAT"])["rangeMin"]);
          d.push("webgl fragment shader low float precision rangeMax:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["LOW_FLOAT"])["rangeMax"]);
          d.push("webgl vertex shader high int precision:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["HIGH_INT"])["precision"]);
          d.push("webgl vertex shader high int precision rangeMin:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["HIGH_INT"])["rangeMin"]);
          d.push("webgl vertex shader high int precision rangeMax:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["HIGH_INT"])["rangeMax"]);
          d.push("webgl vertex shader medium int precision:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["MEDIUM_INT"])["precision"]);
          d.push("webgl vertex shader medium int precision rangeMin:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["MEDIUM_INT"])["rangeMin"]);
          d.push("webgl vertex shader medium int precision rangeMax:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["MEDIUM_INT"])["rangeMax"]);
          d.push("webgl vertex shader low int precision:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["LOW_INT"])["precision"]);
          d.push("webgl vertex shader low int precision rangeMin:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["LOW_INT"])["rangeMin"]);
          d.push("webgl vertex shader low int precision rangeMax:" + e["getShaderPrecisionFormat"](e["VERTEX_SHADER"], e["LOW_INT"])["rangeMax"]);
          d.push("webgl fragment shader high int precision:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["HIGH_INT"])["precision"]);
          d.push("webgl fragment shader high int precision rangeMin:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["HIGH_INT"])["rangeMin"]);
          d.push("webgl fragment shader high int precision rangeMax:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["HIGH_INT"])["rangeMax"]);
          d.push("webgl fragment shader medium int precision:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["MEDIUM_INT"])["precision"]);
          d.push("webgl fragment shader medium int precision rangeMin:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["MEDIUM_INT"])["rangeMin"]);
          d.push("webgl fragment shader medium int precision rangeMax:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["MEDIUM_INT"])["rangeMax"]);
          d.push("webgl fragment shader low int precision:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["LOW_INT"])["precision"]);
          d.push("webgl fragment shader low int precision rangeMin:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["LOW_INT"])["rangeMin"]);
          d.push("webgl fragment shader low int precision rangeMax:" + e["getShaderPrecisionFormat"](e["FRAGMENT_SHADER"], e["LOW_INT"])["rangeMax"]);
          return Da = d;
        }
    const value=(function () {
                var a;
                if (lc()) {
                  a = x["createElement"]("canvas");
                  var e;
                  try {
                    e = a["getContext"] && (a["getContext"]("webgl") || a["getContext"]("experimental-webgl"));
                  } catch (d) {
                    e = !1;
                  }
                  a = !!qd && !!e;
                } else a = !1;
                return a ? za(cd().join("~")) : "";
              })();
    if (hashed) return {kind:'fingerprint',preimage:value};
    if (value === '') return {kind:'unsupported'};
    throw new TypeError('unexpected measurement result');
  } catch (_) { return {kind:'failed'}; }
}
