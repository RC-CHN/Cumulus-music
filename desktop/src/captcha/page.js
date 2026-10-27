(() => {
  'use strict';
  document.body.dataset.bootstrap = 'started';
  let done = false;
  function finish(key, value) {
    if (done) return;
    done = true;
    // Navigation is intercepted by this window's native owner. No IPC or
    // music credentials are present in this document.
    location.href = `/result?${key}=${encodeURIComponent(value)}`;
  }
  function failed() {
    document.body.dataset.phase = 'failed';
    finish('status', 'unavailable');
  }
  document.getElementById('cancel').onclick = () => finish('status', 'cancelled');
  const script = document.createElement('script');
  script.src = 'https://cstaticdun.126.net/load.min.js';
  script.async = true;
  script.onerror = failed;
  script.onload = () => {
    try {
      window.initNECaptcha({
        element: '#captcha', captchaId: '73a18dc827b24b18ad0783701a75277d',
        width: '320px', mode: 'popup', apiVersion: 2, protocol: 'https',
        popupStyles: { position: 'fixed', top: '20%' },
        onVerify(error, data) {
          if (data && typeof data.validate === 'string') finish('validate', data.validate);
          else if (error) failed();
        },
        onClose() { finish('status', 'cancelled'); },
      }, (instance) => {
        if (done) { instance.destroy(); return; }
        document.body.dataset.phase = 'ready';
        document.getElementById('status').textContent = '请完成下方验证码';
        instance.verify();
      }, failed);
    } catch { failed(); }
  };
  document.head.appendChild(script);
})();
