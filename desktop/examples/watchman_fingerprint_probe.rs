//! Real browser measurements -> native Rust fingerprint and wire encoding.
//! Isolated offline example, with no official SDK or remote reporting.
use cumulus_core::{
    account::{AccountSession, profiles::ProfileRegistry},
    client::MusicClient,
    identity::WindowsBrowserIdentity,
    protocol::{
        api::{Api, ApiFuture},
        http::ApiOrigin,
    },
    watchman::{FingerprintReading, SdkBuild, descriptors, encode_field, fingerprint_observation},
};
use cumulus_desktop::watchman::{
    compatibility::context_script,
    measurement::{device_script, fingerprint_script},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, sync::Arc};
use tauri::{Listener, Manager, webview::PageLoadEvent};

struct Offline;
impl Api for Offline {
    fn request<'a>(&'a self, path: &'a str, _: Value, _: bool) -> ApiFuture<'a, Value> {
        Box::pin(async move {
            assert_eq!(path, "/api/w/nuser/account/get");
            Ok(json!({"code":301}))
        })
    }
}

fn main() {
    let root =
        PathBuf::from(std::env::var_os("CUMULUS_PROOF_DIR").expect("new private proof directory"));
    fs::create_dir(&root).unwrap();
    let locales: Vec<String> = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .filter(|s| !s.is_empty())
        .take(1)
        .collect();
    let declaration = WindowsBrowserIdentity::from_locales(&locales, "zh-CN").unwrap();
    let declaration_value = serde_json::to_value(&declaration).unwrap();
    let mut script = format!(
        "(async()=>{{try{{const rows=[],device=[];const declaration={};const projected={};const originalUserAgent=navigator.userAgent,originalPlatform=navigator.platform;",
        declaration_value,
        context_script(&declaration).unwrap()
    );
    for build in [SdkBuild::Af2952a4, SdkBuild::B602a5ad7] {
        for mode in ["actual", "windows_declared"] {
            let context = if mode == "actual" {
                "undefined"
            } else {
                "projected"
            };
            script.push_str(&format!(
                "device.push(...({})({context}).map(row=>({{...row,build:{},mode:{}}})));\n",
                device_script(build),
                serde_json::to_string(build.version()).unwrap(),
                serde_json::to_string(mode).unwrap()
            ));
            for key in ["Jb", "Qa", "dc", "jb"] {
                script.push_str(&format!(
                    "rows.push({{build:{},key:{},mode:{},reading:({})({context})}});\n",
                    serde_json::to_string(build.version()).unwrap(),
                    serde_json::to_string(key).unwrap(),
                    serde_json::to_string(mode).unwrap(),
                    fingerprint_script(build, key).unwrap()
                ));
            }
        }
    }
    script.push_str(r#"
      const languages=navigator.languages;
      const host={userAgent:navigator.userAgent,appVersion:navigator.appVersion,platform:navigator.platform,
        productSub:navigator.productSub,vendor:navigator.vendor,oscpu:navigator.oscpu??null,
        hardwareConcurrency:navigator.hardwareConcurrency,maxTouchPoints:navigator.maxTouchPoints??null,
        evalTextLength:eval.toString().length,timezoneOffset:new Date().getTimezoneOffset(),
        screen:[screen.width,screen.height,screen.availWidth,screen.availHeight],devicePixelRatio:window.devicePixelRatio};
      const node=document.createElement('div');node.id='fingerprint-proof-result';
      node.style.cssText='position:fixed;inset:0;z-index:999999;background:white;color:#222;padding:32px;font:16px sans-serif;overflow:auto';
      const title=document.createElement('h2');title.textContent='本地设备测量验证';node.append(title);
      for(const row of rows.filter(row=>row.mode==='windows_declared')){const line=document.createElement('p');line.textContent=row.build+' / '+row.key+'：'+row.reading.kind;node.append(line);}
      const summary=document.createElement('p');summary.textContent='实际/Windows 声明：'+device.length+' 个设备字段，保留真实引擎检查结果。';node.append(summary);
      const line=document.createElement('p');line.textContent='原始测量交给 Rust 计算指纹；本次没有网络上报。';node.append(line);document.body.append(node);
      await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
      await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'watchman-fingerprint-proof',payload:{rows,device,languages,host,declaration,globalIdentityUnchanged:navigator.userAgent===originalUserAgent&&navigator.platform===originalPlatform}});
    }catch(_){await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'watchman-fingerprint-proof',payload:{failed:true}})}})();
    "#);
    cumulus_desktop::configure(tauri::Builder::default())
        .setup(move |app| {
            let registry=ProfileRegistry::open(&root.join("state"),ApiOrigin::Pc)?;
            let session=Arc::new(AccountSession::new(Arc::new(Offline),Some(registry.store())));
            app.manage(MusicClient::with_session(session,registry));
            app.add_capability(r#"{"identifier":"watchman-fingerprint-proof","windows":["main"],"permissions":["core:event:allow-emit"]}"#)?;
            let handle=app.handle().clone();
            app.listen("watchman-fingerprint-proof",move|event| {
                let run=||->Result<(),Box<dyn std::error::Error>> {
                    let input:Value=serde_json::from_str(event.payload())?;
                    let rows=input["rows"].as_array().ok_or("missing measured rows")?;
                    if rows.len()!=16 || input["globalIdentityUnchanged"]!=true || input["declaration"]!=declaration_value {return Err("incomplete or invalid measurements".into());}
                    let fields=descriptors()?;
                    let mut seen=std::collections::BTreeSet::new();
                    let mut output=vec![];
                    for row in rows {
                        let build=SdkBuild::parse(row["build"].as_str().ok_or("missing build")?)?;
                        let key=row["key"].as_str().ok_or("missing key")?;
                        fingerprint_script(build,key)?;
                        let mode=row["mode"].as_str().filter(|m|["actual","windows_declared"].contains(m)).ok_or("missing measurement mode")?;
                        if !seen.insert((build.version(),key,mode)) {return Err("duplicate measurement".into());}
                        let reading:FingerprintReading=serde_json::from_value(row["reading"].clone())?;
                        let fact=fingerprint_observation(key,reading)?;
                        let field=fields.iter().find(|f|f.key==key).ok_or("missing field")?;
                        output.push(json!({"build":build.version(),"key":key,"mode":mode,"reading":row["reading"],"value":fact.value,"failed":fact.failed,"hex":hex_bytes(&encode_field(field,&fact.value,fact.failed)?)}));
                    }
                    for key in ["Jb","Qa"] {
                        let items:Vec<_>=output.iter().filter(|v|v["key"]==key).collect();
                        if items.iter().any(|v|v["reading"]["kind"]!="fingerprint"||v["value"]!=items[0]["value"]) {return Err("actual plugin/canvas comparison failed".into());}
                    }
                    let device=input["device"].as_array().filter(|d|d.len()==128).ok_or("missing device measurements")?;
                    let mut device_output=vec![];
                    let mut seen=std::collections::BTreeSet::new();
                    for row in device {
                        let build=SdkBuild::parse(row["build"].as_str().ok_or("missing device build")?)?;
                        let mode=row["mode"].as_str().filter(|m|["actual","windows_declared"].contains(m)).ok_or("missing device mode")?;
                        let key=row["key"].as_str().ok_or("missing device key")?;
                        if !seen.insert((build.version(),mode,key)) {return Err("duplicate device field".into());}
                        let field=fields.iter().find(|f|f.key==key).ok_or("invalid device key")?;
                        let failed=row["failed"].as_bool().ok_or("missing device outcome")?;
                        let value=row.get("value").ok_or("undefined device value")?;
                        let mut row=row.clone();
                        row["hex"]=json!(hex_bytes(&encode_field(field,value,failed)?));
                        device_output.push(row);
                    }
                    let mut flags=vec![];
                    for build in [SdkBuild::Af2952a4, SdkBuild::B602a5ad7] {
                        let value=|mode:&str,key:&str|->Result<&Value,Box<dyn std::error::Error>>{device.iter().find(|v|v["build"]==build.version()&&v["mode"]==mode&&v["key"]==key&&v["failed"]==false).map(|v|&v["value"]).ok_or("device field missing or failed".into())};
                        for key in ["Ta","bb","Vb","mb","Nb"] {if value("actual",key)?!=value("windows_declared",key)? {return Err("host fact changed by projection".into());}}
                        for (key,name) in [("$b","userAgent"),("Ia","appVersion"),("Ib","platform"),("zb","language"),("Ab","languages")] {if value("windows_declared",key)?!=&declaration_value[name] {return Err("declared field differs".into());}}
                        flags.push(json!({"build":build.version(),"actualPlatformMismatch":value("actual","pb")?,"declaredPlatformMismatch":value("windows_declared","pb")?,"actualBrowserMismatch":value("actual","ob")?,"declaredBrowserMismatch":value("windows_declared","ob")?}));
                    }
                    let language=fields.iter().find(|f|f.key=="Ab").unwrap();
                    let language_hex=hex_bytes(&encode_field(language,&input["languages"],false)?);
                    fs::write(root.join("measurements.json"),serde_json::to_vec_pretty(&json!({"rows":output,"device":device_output,"languages":input["languages"],"languageHex":language_hex,"host":input["host"],"declaration":declaration_value}))?)?;
                    let outcomes:Vec<_>=output.iter().map(|v|json!({"build":v["build"],"key":v["key"],"kind":v["reading"]["kind"]})).collect();
                    fs::write(root.join("complete.json"),serde_json::to_vec_pretty(&json!({"ok":true,"realWebview":true,"outcomes":outcomes,"deviceFields":device.len(),"compatibilityChecks":flags,"globalIdentityUnchanged":true,"nativeHardwareFactsPreserved":true,"networkRequests":0,"deviceReports":0,"behaviorReports":0,"productionCollectionConnected":false,"realWindowsEnvironment":false}))?)?;
                    let handle=handle.clone();
                    tauri::async_runtime::spawn(async move {tokio::time::sleep(std::time::Duration::from_secs(3)).await;handle.exit(0);});
                    Ok(())
                };
                if let Err(error)=run() {
                    fs::write(root.join("failed.json"),json!({"error":error.to_string()}).to_string()).unwrap();
                    handle.exit(1);
                }
            });
            Ok(())
        })
        .on_page_load(move |webview,payload| {
            if payload.event()==PageLoadEvent::Finished {webview.eval(&script).unwrap();}
        })
        .run(tauri::generate_context!()).expect("real fingerprint measurement proof");
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
