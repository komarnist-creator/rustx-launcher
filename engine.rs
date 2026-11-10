//! Ядро лаунчера: конфиг, загрузки, установка версий и загрузчиков, запуск игры.
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use crate::sys::{self, mc_dir};

pub const ELY_AUTH: &str = "https://authserver.ely.by";
pub const ELY_API: &str = "https://authserver.ely.by/api/authlib-injector";
const MANIFEST_URL: &str = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
const RUNTIME_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

// ---------------------------------------------------------------- конфиг

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub mode: String,
    pub nick: String,
    pub ver: String,
    pub loader: String,
    pub ram: Option<u32>,
    pub close_after: bool,
    pub jvm_extra: String,
    pub win_size: String,
    pub ely_client: String,
    pub ely_token: String,
    pub ely_name: String,
    pub ely_uuid: String,
    pub shortcut_ver: u32,
    pub bg_mode: String,
    pub bg_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: "offline".into(),
            nick: "Player".into(),
            ver: "1.16.5".into(),
            loader: "Fabric".into(),
            ram: None,
            close_after: true,
            jvm_extra: String::new(),
            win_size: String::new(),
            ely_client: String::new(),
            ely_token: String::new(),
            ely_name: String::new(),
            ely_uuid: String::new(),
            shortcut_ver: 0,
            bg_mode: "image".into(),
            bg_path: String::new(),
        }
    }
}

fn cfg_path() -> PathBuf {
    mc_dir().join("rustx.json")
}

pub fn load_cfg() -> Config {
    fs::read_to_string(cfg_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_cfg(c: &Config) {
    let _ = fs::create_dir_all(mc_dir());
    if let Ok(s) = serde_json::to_string_pretty(c) {
        let _ = fs::write(cfg_path(), s);
    }
}

// ---------------------------------------------------------------- прогресс

#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Отменено")
    }
}

impl std::error::Error for Cancelled {}

pub fn is_cancel(e: &anyhow::Error) -> bool {
    e.downcast_ref::<Cancelled>().is_some()
}

pub struct Progress {
    status: Mutex<String>,
    val: AtomicUsize,
    max: AtomicUsize,
    pub cancel: AtomicBool,
    repaint: Box<dyn Fn() + Send + Sync>,
}

impl Progress {
    pub fn new(repaint: Box<dyn Fn() + Send + Sync>) -> Self {
        Progress {
            status: Mutex::new("Готово".to_string()),
            val: AtomicUsize::new(0),
            max: AtomicUsize::new(1),
            cancel: AtomicBool::new(false),
            repaint,
        }
    }

    pub fn say(&self, t: impl Into<String>) {
        *self.status.lock().unwrap() = t.into();
        (self.repaint)();
    }

    pub fn set(&self, v: usize, m: usize) {
        self.max.store(m.max(1), Ordering::SeqCst);
        self.val.store(v, Ordering::SeqCst);
        (self.repaint)();
    }

    pub fn status(&self) -> String {
        self.status.lock().unwrap().clone()
    }

    pub fn frac(&self) -> f32 {
        let m = self.max.load(Ordering::SeqCst).max(1) as f32;
        (self.val.load(Ordering::SeqCst) as f32 / m).clamp(0.0, 1.0)
    }

    pub fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err(anyhow::Error::new(Cancelled));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- HTTP

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(Duration::from_secs(60))
            .user_agent("rustx-launcher/1.0")
            .build()
    })
}

fn http_get(url: &str) -> Result<ureq::Response> {
    agent().get(url).call().map_err(|e| anyhow!("{} -> {}", url, e))
}

fn get_text(url: &str) -> Result<String> {
    let r = http_get(url)?;
    Ok(r.into_string()?)
}

fn get_json(url: &str) -> Result<Value> {
    let r = http_get(url)?;
    Ok(serde_json::from_reader(r.into_reader())?)
}

fn get_bytes(url: &str) -> Result<Vec<u8>> {
    let r = http_get(url)?;
    let mut buf = Vec::new();
    r.into_reader().read_to_end(&mut buf)?;
    Ok(buf)
}

fn post_json(url: &str, data: &Value) -> Result<Value> {
    match agent()
        .post(url)
        .set("Content-Type", "application/json")
        .send_string(&data.to_string())
    {
        Ok(r) => {
            let t = r.into_string()?;
            if t.trim().is_empty() {
                Ok(json!({}))
            } else {
                Ok(serde_json::from_str(&t)?)
            }
        }
        Err(ureq::Error::Status(_, r)) => {
            let t = r.into_string().unwrap_or_default();
            if let Ok(v) = serde_json::from_str::<Value>(&t) {
                let m = v["errorMessage"]
                    .as_str()
                    .or_else(|| v["error"].as_str())
                    .unwrap_or("Ошибка авторизации")
                    .to_string();
                bail!(m);
            }
            bail!("Ошибка сервера")
        }
        Err(e) => bail!("{}", e),
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let s = fs::read_to_string(path).map_err(|e| anyhow!("{}: {}", path.display(), e))?;
    Ok(serde_json::from_str(&s)?)
}

// ---------------------------------------------------------------- загрузка файлов

pub struct Job {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

fn need(job: &Job) -> bool {
    match fs::metadata(&job.path) {
        Ok(m) => match job.size {
            Some(s) => m.len() != s,
            None => m.len() == 0,
        },
        Err(_) => true,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn fetch_once(job: &Job, tmp: &Path) -> Result<()> {
    let resp = http_get(&job.url)?;
    let mut r = resp.into_reader();
    let mut f = fs::File::create(tmp)?;
    let mut h = Sha1::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n])?;
        h.update(&buf[..n]);
    }
    drop(f);
    if let Some(s) = &job.sha1 {
        let got = hex(&h.finalize());
        if !got.eq_ignore_ascii_case(s) {
            bail!("Контрольная сумма не совпала: {}", job.url);
        }
    }
    Ok(())
}

fn fetch(job: &Job) -> Result<()> {
    if let Some(p) = job.path.parent() {
        fs::create_dir_all(p)?;
    }
    let mut tmp_name = job.path.file_name().map(|s| s.to_os_string()).unwrap_or_default();
    tmp_name.push(".part");
    let tmp = job.path.with_file_name(tmp_name);
    let mut last = anyhow!("загрузка не удалась");
    for _ in 0..3 {
        match fetch_once(job, &tmp) {
            Ok(()) => {
                fs::rename(&tmp, &job.path)?;
                return Ok(());
            }
            Err(e) => last = e,
        }
    }
    let _ = fs::remove_file(&tmp);
    Err(last)
}

/// Параллельная загрузка (8 потоков). Уже скачанные файлы пропускаются.
fn download_all(jobs: Vec<Job>, p: &Progress) -> Result<()> {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let jobs: Vec<Job> = jobs
        .into_iter()
        .filter(|j| seen.insert(j.path.clone()))
        .filter(need)
        .collect();
    let total = jobs.len();
    if total == 0 {
        return Ok(());
    }
    p.set(0, total);
    let queue = Mutex::new(jobs.into_iter());
    let err: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let done = AtomicUsize::new(0);
    let workers = 8.min(total);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                if p.cancel.load(Ordering::SeqCst) || err.lock().unwrap().is_some() {
                    break;
                }
                let job = { queue.lock().unwrap().next() };
                let job = match job {
                    Some(j) => j,
                    None => break,
                };
                match fetch(&job) {
                    Ok(()) => {
                        let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                        p.set(d, total);
                    }
                    Err(e) => {
                        *err.lock().unwrap() = Some(e);
                        break;
                    }
                }
            });
        }
    });
    p.check()?;
    if let Some(e) = err.into_inner().unwrap() {
        return Err(e);
    }
    Ok(())
}

// ---------------------------------------------------------------- версии

pub fn release_versions() -> Result<Vec<String>> {
    let v = get_json(MANIFEST_URL)?;
    Ok(v["versions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|x| x["type"] == "release")
                .filter_map(|x| x["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default())
}

fn versions_dir(id: &str) -> PathBuf {
    mc_dir().join("versions").join(id)
}

fn version_json_path(id: &str) -> PathBuf {
    versions_dir(id).join(format!("{id}.json"))
}

pub fn installed_versions() -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(rd) = fs::read_dir(mc_dir().join("versions")) {
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            if e.path().join(format!("{id}.json")).exists() {
                v.push(id);
            }
        }
    }
    v.sort();
    v
}

fn flag_path(mc: &str, loader: &str) -> PathBuf {
    mc_dir().join(format!(".rustx_incomplete_{mc}_{loader}"))
}

pub fn find_loader_id(mc: &str, loader: &str) -> Option<String> {
    let ids = installed_versions();
    let tail = format!("-{mc}");
    match loader {
        "Fabric" => ids
            .into_iter()
            .rev()
            .find(|i| i.starts_with("fabric-loader") && i.ends_with(&tail)),
        "Quilt" => ids
            .into_iter()
            .rev()
            .find(|i| i.starts_with("quilt-loader") && i.ends_with(&tail)),
        "Forge" => ids
            .into_iter()
            .rev()
            .find(|i| i.to_lowercase().contains("forge") && i.starts_with(mc)),
        _ => Some(mc.to_string()),
    }
}

pub fn is_installed(mc: &str, loader: &str) -> bool {
    if flag_path(mc, loader).exists() {
        return false;
    }
    let ids = installed_versions();
    if !ids.iter().any(|i| i == mc) {
        return false;
    }
    find_loader_id(mc, loader).is_some()
}

fn ensure_vanilla_json(mc: &str, p: &Progress) -> Result<()> {
    let path = version_json_path(mc);
    if path.exists() {
        return Ok(());
    }
    p.say("Получение списка версий...");
    let m = get_json(MANIFEST_URL)?;
    let ent = m["versions"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["id"] == mc))
        .ok_or_else(|| anyhow!("Версия {} не найдена", mc))?;
    let url = ent["url"].as_str().ok_or_else(|| anyhow!("У версии нет адреса"))?;
    let txt = get_text(url)?;
    fs::create_dir_all(versions_dir(mc))?;
    fs::write(&path, txt)?;
    Ok(())
}

/// JSON версии с учётом inheritsFrom (Fabric/Quilt/Forge наследуют ванильную).
fn load_merged(id: &str) -> Result<Value> {
    let mut v = read_json(&version_json_path(id))?;
    let parent = v["inheritsFrom"].as_str().map(String::from);
    match parent {
        Some(pid) => {
            let p = load_merged(&pid)?;
            Ok(merge(p, v))
        }
        None => {
            v["_jar_id"] = json!(id);
            Ok(v)
        }
    }
}

fn merge(parent: Value, child: Value) -> Value {
    let mut out = parent.clone();
    if let (Some(obj), Some(cobj)) = (out.as_object_mut(), child.as_object()) {
        for (k, val) in cobj {
            match k.as_str() {
                "libraries" => {
                    let mut libs = val.as_array().cloned().unwrap_or_default();
                    libs.extend(parent["libraries"].as_array().cloned().unwrap_or_default());
                    obj.insert(k.clone(), Value::Array(libs));
                }
                "arguments" => {
                    let mut args = parent["arguments"].clone();
                    if !args.is_object() {
                        args = json!({});
                    }
                    for key in ["game", "jvm"] {
                        let mut a = args[key].as_array().cloned().unwrap_or_default();
                        a.extend(val[key].as_array().cloned().unwrap_or_default());
                        args[key] = Value::Array(a);
                    }
                    obj.insert(k.clone(), args);
                }
                "inheritsFrom" => {}
                _ => {
                    obj.insert(k.clone(), val.clone());
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- библиотеки и правила

fn rule_applies(r: &Value) -> bool {
    if let Some(os) = r.get("os") {
        if let Some(n) = os["name"].as_str() {
            if n != sys::OS_NAME {
                return false;
            }
        }
        if let Some(a) = os["arch"].as_str() {
            if a != sys::OS_ARCH {
                return false;
            }
        }
    }
    if let Some(f) = r.get("features").and_then(|f| f.as_object()) {
        // демо-режим и своё разрешение не включаем: правило с включённой фичей не подходит
        for (_, want) in f {
            if want.as_bool().unwrap_or(false) {
                return false;
            }
        }
    }
    true
}

fn rules_ok(rules: &Value) -> bool {
    let arr = match rules.as_array() {
        Some(a) => a,
        None => return true,
    };
    if arr.is_empty() {
        return true;
    }
    let mut allowed = false;
    for r in arr {
        if rule_applies(r) {
            allowed = r["action"] == "allow";
        }
    }
    allowed
}

fn maven_path(name: &str) -> Option<String> {
    let (coord, ext) = match name.split_once('@') {
        Some((c, e)) => (c, e),
        None => (name, "jar"),
    };
    let p: Vec<&str> = coord.split(':').collect();
    if p.len() < 3 {
        return None;
    }
    let g = p[0].replace('.', "/");
    let (a, v) = (p[1], p[2]);
    let file = if p.len() > 3 {
        format!("{a}-{v}-{}.{ext}", p[3])
    } else {
        format!("{a}-{v}.{ext}")
    };
    Some(format!("{g}/{a}/{v}/{file}"))
}

fn dedupe_key(name: &str) -> String {
    let p: Vec<&str> = name.split(':').collect();
    if p.len() >= 2 {
        let cls = if p.len() > 3 { p[3] } else { "" };
        format!("{}:{}:{}", p[0], p[1], cls)
    } else {
        name.to_string()
    }
}

struct LibPlan {
    classpath: Vec<PathBuf>,
    jobs: Vec<Job>,
    natives: Vec<(PathBuf, Vec<String>)>,
}

fn plan_libs(v: &Value) -> LibPlan {
    let libs_dir = mc_dir().join("libraries");
    let mut plan = LibPlan { classpath: Vec::new(), jobs: Vec::new(), natives: Vec::new() };
    let mut seen: HashSet<String> = HashSet::new();
    let empty: Vec<Value> = Vec::new();
    let list = v["libraries"].as_array().unwrap_or(&empty);
    for lib in list {
        if !rules_ok(&lib["rules"]) {
            continue;
        }
        let name = lib["name"].as_str().unwrap_or("");
        let dl = &lib["downloads"];
        let art = &dl["artifact"];

        let mut entry: Option<(String, String, Option<String>, Option<u64>)> = None;
        if art.is_object() {
            let rel = art["path"].as_str().map(String::from).or_else(|| maven_path(name));
            if let Some(rel) = rel {
                entry = Some((
                    rel,
                    art["url"].as_str().unwrap_or("").to_string(),
                    art["sha1"].as_str().map(String::from),
                    art["size"].as_u64(),
                ));
            }
        } else if dl.is_null() {
            // стиль Fabric/Quilt: только имя и базовый адрес maven
            if let Some(rel) = maven_path(name) {
                let base = lib["url"].as_str().unwrap_or("https://libraries.minecraft.net/");
                let base = if base.ends_with('/') { base.to_string() } else { format!("{base}/") };
                entry = Some((
                    rel.clone(),
                    format!("{base}{rel}"),
                    lib["sha1"].as_str().map(String::from),
                    lib["size"].as_u64(),
                ));
            }
        }
        if let Some((rel, url, sha1, size)) = entry {
            if seen.insert(dedupe_key(name)) {
                let path = libs_dir.join(&rel);
                plan.classpath.push(path.clone());
                if !url.is_empty() {
                    plan.jobs.push(Job { url, path, sha1, size });
                }
            }
        }

        // старый стиль natives: отдельный архив, который распаковывается в папку natives
        if let Some(nat) = lib["natives"][sys::OS_NAME].as_str() {
            let arch = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
            let cls = nat.replace("${arch}", arch);
            let c = &dl["classifiers"][cls.as_str()];
            if let Some(rel) = c["path"].as_str() {
                let path = libs_dir.join(rel);
                if let Some(url) = c["url"].as_str() {
                    if !url.is_empty() {
                        plan.jobs.push(Job {
                            url: url.to_string(),
                            path: path.clone(),
                            sha1: c["sha1"].as_str().map(String::from),
                            size: c["size"].as_u64(),
                        });
                    }
                }
                let excl: Vec<String> = lib["extract"]["exclude"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                plan.natives.push((path, excl));
            }
        }
    }
    plan
}

fn extract_natives(list: &[(PathBuf, Vec<String>)], dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    for (jar, excl) in list {
        let f = match fs::File::open(jar) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let mut z = zip::ZipArchive::new(f)?;
        for i in 0..z.len() {
            let mut e = z.by_index(i)?;
            if e.is_dir() {
                continue;
            }
            let name = e.name().to_string();
            if excl.iter().any(|x| name.starts_with(x.as_str())) {
                continue;
            }
            let rel = match e.enclosed_name() {
                Some(r) => r,
                None => continue,
            };
            let out = dir.join(rel);
            if out.exists() {
                continue;
            }
            if let Some(par) = out.parent() {
                fs::create_dir_all(par)?;
            }
            let mut o = fs::File::create(&out)?;
            std::io::copy(&mut e, &mut o)?;
        }
    }
    Ok(())
}

/// Скачивает клиент, библиотеки и ресурсы версии, распаковывает natives.
fn install_game_files(vid: &str, p: &Progress) -> Result<()> {
    let v = load_merged(vid)?;
    let jar_id = v["_jar_id"].as_str().unwrap_or(vid).to_string();
    let plan = plan_libs(&v);
    let mut jobs = plan.jobs;
    let client = &v["downloads"]["client"];
    if let Some(url) = client["url"].as_str() {
        jobs.push(Job {
            url: url.to_string(),
            path: versions_dir(&jar_id).join(format!("{jar_id}.jar")),
            sha1: client["sha1"].as_str().map(String::from),
            size: client["size"].as_u64(),
        });
    }
    p.say("Загрузка библиотек...");
    download_all(jobs, p)?;
    p.check()?;

    let ai = &v["assetIndex"];
    if let (Some(id), Some(url)) = (ai["id"].as_str(), ai["url"].as_str()) {
        let idx_path = mc_dir().join("assets").join("indexes").join(format!("{id}.json"));
        download_all(
            vec![Job {
                url: url.to_string(),
                path: idx_path.clone(),
                sha1: ai["sha1"].as_str().map(String::from),
                size: ai["size"].as_u64(),
            }],
            p,
        )?;
        let idx = read_json(&idx_path)?;
        let mut aj: Vec<Job> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        if let Some(objs) = idx["objects"].as_object() {
            for (_, o) in objs {
                if let Some(h) = o["hash"].as_str() {
                    if h.len() < 2 || !seen.insert(h.to_string()) {
                        continue;
                    }
                    let pre = &h[..2];
                    aj.push(Job {
                        url: format!("https://resources.download.minecraft.net/{pre}/{h}"),
                        path: mc_dir().join("assets").join("objects").join(pre).join(h),
                        sha1: Some(h.to_string()),
                        size: o["size"].as_u64(),
                    });
                }
            }
        }
        p.say("Загрузка ресурсов...");
        download_all(aj, p)?;
    }
    p.check()?;
    extract_natives(&plan.natives, &versions_dir(vid).join("natives"))?;
    Ok(())
}

// ---------------------------------------------------------------- Java от Mojang

fn install_runtime(comp: &str, plat: &str, root: &Path, p: &Progress) -> Result<()> {
    let all = get_json(RUNTIME_URL)?;
    let ent = &all[plat][comp][0];
    let murl = ent["manifest"]["url"]
        .as_str()
        .ok_or_else(|| anyhow!("Java {} недоступна для {}", comp, plat))?;
    let man = get_json(murl)?;
    let files = man["files"].as_object().ok_or_else(|| anyhow!("Плохой манифест Java"))?;
    let mut jobs: Vec<Job> = Vec::new();
    for (rel, f) in files {
        match f["type"].as_str() {
            Some("directory") => {
                fs::create_dir_all(root.join(rel))?;
            }
            Some("file") => {
                let raw = &f["downloads"]["raw"];
                if let Some(url) = raw["url"].as_str() {
                    jobs.push(Job {
                        url: url.to_string(),
                        path: root.join(rel),
                        sha1: raw["sha1"].as_str().map(String::from),
                        size: raw["size"].as_u64(),
                    });
                }
            }
            _ => {}
        }
    }
    download_all(jobs, p)
}

/// Возвращает (javaw для игры, java для установщиков).
fn ensure_java(v: &Value, p: &Progress) -> Result<(PathBuf, PathBuf)> {
    let comp = v["javaVersion"]["component"].as_str().unwrap_or("jre-legacy").to_string();
    let plat = sys::RUNTIME_PLATFORM;
    let root = mc_dir().join("runtime").join(&comp).join(plat).join(&comp);
    let exe = root.join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
    let marker = root.join(".rustx_ok");
    if !marker.exists() {
        p.say("Загрузка Java...");
        match install_runtime(&comp, plat, &root, p) {
            Ok(()) => {
                let _ = fs::write(&marker, b"ok");
            }
            Err(e) => {
                if is_cancel(&e) || !exe.exists() {
                    return Err(e);
                }
            }
        }
    }
    let gui = if cfg!(windows) {
        let w = root.join("bin").join("javaw.exe");
        if w.exists() { w } else { exe.clone() }
    } else {
        exe.clone()
    };
    Ok((gui, exe))
}

// ---------------------------------------------------------------- загрузчики

fn meta_install(base: &str, mc: &str, by_flag: bool, name: &str) -> Result<()> {
    let list = get_json(&format!("{base}/{mc}"))?;
    let arr = list
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| anyhow!("Для версии {} нет {}", mc, name))?;
    let pick = arr
        .iter()
        .find(|x| {
            if by_flag {
                x["loader"]["stable"] == true
            } else {
                !x["loader"]["version"].as_str().unwrap_or("-").contains('-')
            }
        })
        .unwrap_or(&arr[0]);
    let lv = pick["loader"]["version"]
        .as_str()
        .ok_or_else(|| anyhow!("Нет версии загрузчика {}", name))?;
    let txt = get_text(&format!("{base}/{mc}/{lv}/profile/json"))?;
    let j: Value = serde_json::from_str(&txt)?;
    let id = j["id"].as_str().ok_or_else(|| anyhow!("Плохой профиль {}", name))?.to_string();
    fs::create_dir_all(versions_dir(&id))?;
    fs::write(version_json_path(&id), txt)?;
    Ok(())
}

fn install_forge(mc: &str, java_cli: &Path, p: &Progress) -> Result<()> {
    p.say("Поиск Forge...");
    let promos = get_json("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")?;
    let pr = &promos["promos"];
    let fv = pr[format!("{mc}-latest").as_str()]
        .as_str()
        .or_else(|| pr[format!("{mc}-recommended").as_str()].as_str())
        .ok_or_else(|| anyhow!("Для версии {} нет Forge", mc))?
        .to_string();
    let full = format!("{mc}-{fv}");
    let url = format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{full}/forge-{full}-installer.jar"
    );
    let tmp_dir = mc_dir().join("rustx_tmp");
    let inst = tmp_dir.join(format!("forge-{full}-installer.jar"));
    p.say("Загрузка установщика Forge...");
    download_all(vec![Job { url, path: inst.clone(), sha1: None, size: None }], p)?;
    p.check()?;

    let profiles = mc_dir().join("launcher_profiles.json");
    if !profiles.exists() {
        fs::write(&profiles, r#"{"profiles":{},"settings":{},"version":3}"#)?;
    }
    p.say("Установка Forge (может занять пару минут)...");
    p.set(0, 1);
    let mut cmd = Command::new(java_cli);
    cmd.arg("-jar")
        .arg(&inst)
        .arg("--installClient")
        .arg(mc_dir())
        .current_dir(mc_dir())
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(sys::NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| anyhow!("Не удалось запустить установщик Forge: {}", e))?;
    let _ = fs::remove_dir_all(&tmp_dir);
    if !out.status.success() {
        let mut text = String::from_utf8_lossy(&out.stderr).to_string();
        if text.trim().is_empty() {
            text = String::from_utf8_lossy(&out.stdout).to_string();
        }
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(6)..].join("\n");
        bail!("Установщик Forge завершился с ошибкой:\n{}", tail);
    }
    Ok(())
}

/// Ставит версию, Java и загрузчик. Возвращает (id версии для запуска, javaw).
pub fn prepare(mc: &str, loader: &str, p: &Progress) -> Result<(String, PathBuf)> {
    let flag = flag_path(mc, loader);
    if !is_installed(mc, loader) {
        fs::create_dir_all(mc_dir())?;
        fs::write(&flag, b"")?;
    }
    p.check()?;
    ensure_vanilla_json(mc, p)?;
    let vanilla = load_merged(mc)?;
    let (java_gui, java_cli) = ensure_java(&vanilla, p)?;
    p.check()?;
    install_game_files(mc, p)?;

    let vid = match loader {
        "Fabric" => {
            if find_loader_id(mc, "Fabric").is_none() {
                p.say("Установка Fabric...");
                meta_install("https://meta.fabricmc.net/v2/versions/loader", mc, true, "Fabric")?;
            }
            find_loader_id(mc, "Fabric").ok_or_else(|| anyhow!("Не удалось установить Fabric"))?
        }
        "Quilt" => {
            if find_loader_id(mc, "Quilt").is_none() {
                p.say("Установка Quilt...");
                meta_install("https://meta.quiltmc.org/v3/versions/loader", mc, false, "Quilt")?;
            }
            find_loader_id(mc, "Quilt").ok_or_else(|| anyhow!("Не удалось установить Quilt"))?
        }
        "Forge" => {
            if find_loader_id(mc, "Forge").is_none() {
                install_forge(mc, &java_cli, p)?;
            }
            find_loader_id(mc, "Forge").ok_or_else(|| anyhow!("Не удалось установить Forge"))?
        }
        _ => mc.to_string(),
    };
    p.check()?;
    if vid != mc {
        install_game_files(&vid, p)?;
    }
    let _ = fs::remove_file(&flag);
    Ok((vid, java_gui))
}

// ---------------------------------------------------------------- запуск

pub struct Launch {
    pub username: String,
    pub uuid: String,
    pub token: String,
    pub user_jvm: Vec<String>,
    pub java: PathBuf,
}

fn subst(s: &str, vars: &HashMap<&str, String>) -> String {
    let mut r = s.to_string();
    for (k, v) in vars {
        r = r.replace(&format!("${{{}}}", k), v);
    }
    r
}

fn expand_args(list: &Value, vars: &HashMap<&str, String>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(arr) = list.as_array() {
        for a in arr {
            if let Some(s) = a.as_str() {
                out.push(subst(s, vars));
            } else if a.is_object() {
                if !rules_ok(&a["rules"]) {
                    continue;
                }
                match &a["value"] {
                    Value::String(s) => out.push(subst(s, vars)),
                    Value::Array(vs) => {
                        for x in vs {
                            if let Some(s) = x.as_str() {
                                out.push(subst(s, vars));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

fn build_command(vid: &str, l: &Launch) -> Result<Vec<String>> {
    let v = load_merged(vid)?;
    let plan = plan_libs(&v);
    let jar_id = v["_jar_id"].as_str().unwrap_or(vid).to_string();
    let jar = versions_dir(&jar_id).join(format!("{jar_id}.jar"));
    let mut cp: Vec<String> = plan.classpath.iter().map(|p| p.to_string_lossy().to_string()).collect();
    cp.push(jar.to_string_lossy().to_string());
    let sep = if cfg!(windows) { ";" } else { ":" };
    let classpath = cp.join(sep);
    let natives = versions_dir(vid).join("natives");
    let mc = mc_dir();

    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", l.username.clone());
    vars.insert("version_name", vid.to_string());
    vars.insert("game_directory", mc.to_string_lossy().to_string());
    vars.insert("assets_root", mc.join("assets").to_string_lossy().to_string());
    vars.insert("assets_index_name", v["assetIndex"]["id"].as_str().unwrap_or("legacy").to_string());
    vars.insert("auth_uuid", l.uuid.clone());
    vars.insert("auth_access_token", l.token.clone());
    vars.insert("auth_session", l.token.clone());
    vars.insert("clientid", String::new());
    vars.insert("auth_xuid", String::new());
    vars.insert("user_type", "mojang".to_string());
    vars.insert("version_type", v["type"].as_str().unwrap_or("release").to_string());
    vars.insert("user_properties", "{}".to_string());
    vars.insert("natives_directory", natives.to_string_lossy().to_string());
    vars.insert("launcher_name", "rustx".to_string());
    vars.insert("launcher_version", "1.0".to_string());
    vars.insert("classpath", classpath.clone());
    vars.insert("classpath_separator", sep.to_string());
    vars.insert("library_directory", mc.join("libraries").to_string_lossy().to_string());
    vars.insert("game_assets", mc.join("assets").join("virtual").join("legacy").to_string_lossy().to_string());

    let mut jvm = expand_args(&v["arguments"]["jvm"], &vars);
    let game: Vec<String> = if v["arguments"]["game"].is_array() {
        expand_args(&v["arguments"]["game"], &vars)
    } else if let Some(s) = v["minecraftArguments"].as_str() {
        s.split_whitespace().map(|a| subst(a, &vars)).collect()
    } else {
        Vec::new()
    };
    if !jvm.iter().any(|a| a == "-cp" || a == "-classpath") {
        jvm.push("-cp".to_string());
        jvm.push(classpath);
    }
    if !jvm.iter().any(|a| a.starts_with("-Djava.library.path")) {
        jvm.push(format!("-Djava.library.path={}", natives.display()));
    }
    let main_class = v["mainClass"].as_str().ok_or_else(|| anyhow!("В версии нет mainClass"))?.to_string();

    let mut args = l.user_jvm.clone();
    args.extend(jvm);
    args.push(main_class);
    args.extend(game);
    Ok(args)
}

fn spawn_game(java: &Path, args: &[String], flags: u32) -> std::io::Result<std::process::Child> {
    let mut cmd = Command::new(java);
    cmd.args(args)
        .current_dir(mc_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(flags);
    }
    #[cfg(not(windows))]
    {
        let _ = flags;
    }
    cmd.spawn()
}

/// Полный цикл «ИГРАТЬ»: установка (если нужна), настройки, аккаунт, запуск.
pub fn run_game(
    cfg: &mut Config,
    mode: &str,
    nick: &str,
    mc: &str,
    loader: &str,
    ram: u32,
    p: &Progress,
) -> Result<()> {
    let (vid, java) = prepare(mc, loader, p)?;
    apply_profile();

    let mut jvm = vec![format!("-Xmx{ram}M"), format!("-Xms{}M", ram.min(2048))];
    jvm.extend(jvm_flags());

    let (name, uuid, token) = if mode == "ely" {
        p.say("Проверка Ely.by...");
        ely_refresh(cfg)?;
        let inj = ensure_injector()?;
        jvm.push(format!("-javaagent:{}={}", inj.display(), ELY_API));
        (cfg.ely_name.clone(), cfg.ely_uuid.clone(), cfg.ely_token.clone())
    } else {
        let n = nick.trim().to_string();
        let u = offline_uuid(&n);
        (n, u, "0".to_string())
    };
    jvm.extend(split_args(&cfg.jvm_extra));

    let launch = Launch { username: name, uuid, token, user_jvm: jvm, java: java.clone() };
    let args = build_command(&vid, &launch)?;
    p.say("Запуск игры...");
    p.set(0, 1);
    let flags = sys::NO_WINDOW | 0x0000_8000; // CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB
    spawn_game(&java, &args, flags)
        .or_else(|_| spawn_game(&java, &args, sys::NO_WINDOW))
        .map_err(|e| anyhow!("Не удалось запустить игру: {}", e))?;
    Ok(())
}

// ---------------------------------------------------------------- аккаунты

fn random_hex() -> String {
    let mut h = Sha1::new();
    h.update(format!("{:?}{}", std::time::SystemTime::now(), std::process::id()).as_bytes());
    hex(&h.finalize()[..16])
}

pub fn ely_login(login: &str, password: &str, cfg: &mut Config) -> Result<()> {
    let client = if cfg.ely_client.is_empty() { random_hex() } else { cfg.ely_client.clone() };
    let data = post_json(
        &format!("{ELY_AUTH}/auth/authenticate"),
        &json!({
            "username": login,
            "password": password,
            "clientToken": client,
            "requestUser": true
        }),
    )?;
    let prof = &data["selectedProfile"];
    match (data["accessToken"].as_str(), prof["name"].as_str(), prof["id"].as_str()) {
        (Some(t), Some(n), Some(i)) => {
            cfg.ely_client = client;
            cfg.ely_token = t.to_string();
            cfg.ely_name = n.to_string();
            cfg.ely_uuid = i.to_string();
            save_cfg(cfg);
            Ok(())
        }
        _ => bail!("У этого аккаунта Ely.by нет профиля Minecraft"),
    }
}

pub fn ely_refresh(cfg: &mut Config) -> Result<()> {
    let data = post_json(
        &format!("{ELY_AUTH}/auth/refresh"),
        &json!({"accessToken": cfg.ely_token, "clientToken": cfg.ely_client}),
    )?;
    if let Some(t) = data["accessToken"].as_str() {
        cfg.ely_token = t.to_string();
        save_cfg(cfg);
    }
    Ok(())
}

fn ensure_injector() -> Result<PathBuf> {
    let path = mc_dir().join("authlib-injector.jar");
    if path.exists() {
        return Ok(path);
    }
    let info = get_json("https://authlib-injector.yushi.moe/artifact/latest.json")?;
    let url = info["download_url"].as_str().ok_or_else(|| anyhow!("Нет ссылки на authlib-injector"))?;
    fetch(&Job { url: url.to_string(), path: path.clone(), sha1: None, size: None })?;
    Ok(path)
}

pub fn offline_uuid(name: &str) -> String {
    let mut h = md5::compute(format!("OfflinePlayer:{name}").as_bytes()).0;
    h[6] = (h[6] & 0x0f) | 0x30;
    h[8] = (h[8] & 0x3f) | 0x80;
    let s = hex(&h);
    format!("{}-{}-{}-{}-{}", &s[0..8], &s[8..12], &s[12..16], &s[16..20], &s[20..32])
}

/// Лицо скина с Ely.by: 32x32 RGBA.
pub fn fetch_avatar(name: &str) -> Option<Vec<u8>> {
    let bytes = get_bytes(&format!("https://skinsystem.ely.by/skins/{name}.png")).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    if img.width() < 16 || img.height() < 16 {
        return None;
    }
    let mut out = Vec::with_capacity(32 * 32 * 4);
    for y in 0..32u32 {
        for x in 0..32u32 {
            let px = img.get_pixel(8 + x / 4, 8 + y / 4);
            out.extend_from_slice(&px.0);
        }
    }
    Some(out)
}

// ---------------------------------------------------------------- память, флаги, настройки игры

/// (мин, макс разумного объёма игре в МБ, всего ОЗУ в МБ)
pub fn ram_advice() -> (u32, u32, u32) {
    let total = sys::ram_info().map(|r| r.0 as u32).unwrap_or(4096);
    let (lo, hi) = if total <= 4500 {
        (2560, 3072)
    } else if total <= 6500 {
        (3072, 4096)
    } else if total <= 9000 {
        (4096, 5120)
    } else {
        (4096, 6144)
    };
    (lo, hi, total)
}

pub fn default_ram() -> u32 {
    ram_advice().1
}

fn jvm_flags() -> Vec<String> {
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let par = (cpus / 2).clamp(2, 8);
    vec![
        "-XX:+UseG1GC".to_string(),
        "-XX:+UnlockExperimentalVMOptions".to_string(),
        "-XX:G1NewSizePercent=20".to_string(),
        "-XX:G1ReservePercent=20".to_string(),
        "-XX:MaxGCPauseMillis=30".to_string(),
        "-XX:+DisableExplicitGC".to_string(),
        format!("-XX:ParallelGCThreads={par}"),
        format!("-XX:ConcGCThreads={}", (par / 4).max(1)),
        "-Djava.net.preferIPv4Stack=true".to_string(),
    ]
}

pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in s.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                    has = true;
                } else if c.is_whitespace() {
                    if has || !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                        has = false;
                    }
                } else {
                    cur.push(c);
                }
            }
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Профиль «PvP-Эко (слабый ноутбук)»: записывается в options.txt перед запуском.
const PROFILE: &[(&str, &str)] = &[
    ("renderDistance", "6"),
    ("maxFps", "260"),
    ("particles", "2"),
    ("fancyGraphics", "false"),
    ("ao", "0"),
    ("entityShadows", "false"),
    ("renderClouds", "false"),
    ("biomeBlendRadius", "0"),
    ("mipmapLevels", "2"),
    ("entityDistanceScaling", "0.5"),
    ("enableVsync", "false"),
    ("rawMouseInput", "true"),
    ("autoJump", "false"),
    ("attackIndicator", "1"),
];

fn apply_profile() {
    let path = mc_dir().join("options.txt");
    let mut lines: Vec<String> = fs::read_to_string(&path)
        .map(|s| s.lines().map(String::from).collect())
        .unwrap_or_default();
    let mut done: HashSet<String> = HashSet::new();
    for ln in lines.iter_mut() {
        let key = ln.split(':').next().unwrap_or("").to_string();
        if let Some(&(_, val)) = PROFILE.iter().find(|(pk, _)| *pk == key.as_str()) {
            *ln = format!("{key}:{val}");
            done.insert(key);
        }
    }
    for &(k, v) in PROFILE {
        if !done.contains(k) {
            lines.push(format!("{k}:{v}"));
        }
    }
    let _ = fs::create_dir_all(mc_dir());
    let _ = fs::write(&path, lines.join("\n") + "\n");
}

pub fn dir_size(sub: &str) -> u64 {
    fn walk(p: &Path) -> u64 {
        let mut total = 0;
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    total += walk(&path);
                } else if let Ok(m) = e.metadata() {
                    total += m.len();
                }
            }
        }
        total
    }
    walk(&mc_dir().join(sub))
}

pub fn clear_logs() {
    for d in ["logs", "crash-reports"] {
        let p = mc_dir().join(d);
        let _ = fs::remove_dir_all(&p);
        let _ = fs::create_dir_all(&p);
    }
}
