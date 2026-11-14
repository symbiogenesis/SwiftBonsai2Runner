//! Resumable publication with one mutation attempt and authoritative readback.
use super::{Result, response, text, validate};
use crate::release_tool::{assets, path, process, resolve, version};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

type Transport<'a> = dyn FnMut(&str, &[String], u64) -> Result<String> + 'a;

struct Publication {
    repository_root: PathBuf,
    policy: Value,
}

impl Publication {
    fn repository(&self) -> Result<&str> {
        text(&self.policy, "repository")
    }
    fn tag(&self) -> Result<&str> {
        text(&self.policy, "tag")
    }
    fn revision(&self) -> Result<&str> {
        text(&self.policy, "sourceRevision")
    }
    fn url(&self) -> Result<String> {
        Ok(format!(
            "https://github.com/{}/releases/tag/{}",
            self.repository()?,
            self.tag()?
        ))
    }
    fn endpoint(&self, suffix: &str) -> Result<String> {
        Ok(format!("repos/{}/{suffix}", self.repository()?))
    }
    fn check(&self, release: &Value, draft: bool, id: u64, complete: bool) -> Result<()> {
        let mut policy = self.policy.clone();
        policy["release"] = release.clone();
        policy["expectedDraft"] = draft.into();
        policy["expectedId"] = id.into();
        policy["complete"] = complete.into();
        validate(&policy)
    }
    fn get(&self, transport: &mut Transport<'_>, id: u64) -> Result<Value> {
        response(&transport(
            "gh",
            &["api".into(), self.endpoint(&format!("releases/{id}"))?],
            180,
        )?)
    }
    fn find(&self, transport: &mut Transport<'_>) -> Result<Option<Value>> {
        let raw = transport(
            "gh",
            &[
                "api".into(),
                self.endpoint("releases?per_page=100")?,
                "--paginate".into(),
                "--jq".into(),
                ".[] | {id, tag_name} | @json".into(),
            ],
            180,
        )?;
        let id = super::catalog(&json!({"text":raw,"tag":self.tag()?}))?;
        match id.as_u64() {
            Some(id) => {
                let release = self.get(transport, id)?;
                if super::id(&release["id"])? != id {
                    return Err("Release identity differs from catalog entry.".into());
                }
                Ok(Some(release))
            }
            None => Ok(None),
        }
    }
    fn confirm_tag(&self, transport: &mut Transport<'_>, required: bool) -> Result<()> {
        let raw = transport(
            "gh",
            &[
                "api".into(),
                self.endpoint(&format!("git/matching-refs/tags/{}", self.tag()?))?,
            ],
            180,
        )?;
        let mut object = super::tag(
            &json!({"text":raw,"tag":self.tag()?,"sourceRevision":self.revision()?,"requireExisting":required}),
        )?;
        for depth in 1..=8 {
            if object.is_null() {
                return Ok(());
            }
            let annotation = response(&transport(
                "gh",
                &[
                    "api".into(),
                    self.endpoint(&format!("git/tags/{}", text(&object, "sha")?))?,
                ],
                180,
            )?)?;
            object = super::tag_step(
                &json!({"object":annotation["object"],"depth":depth,"sourceRevision":self.revision()?}),
            )?;
        }
        if object.is_null() {
            Ok(())
        } else {
            Err("Release tag nesting exceeds bound.".into())
        }
    }
    fn main_matches(&self, transport: &mut Transport<'_>) -> Result<()> {
        let main = transport(
            "gh",
            &[
                "api".into(),
                self.endpoint("commits/main")?,
                "--jq".into(),
                ".sha".into(),
            ],
            180,
        )?;
        if main.trim() != self.revision()? {
            return Err("Remote main advanced; publication requires settled source.".into());
        }
        Ok(())
    }
    fn existing(&self, transport: &mut Transport<'_>, release: &Value) -> Result<Value> {
        self.check(release, false, 0, true)?;
        self.confirm_tag(transport, true)?;
        self.check(
            &self.get(transport, super::id(&release["id"])?)?,
            false,
            super::id(&release["id"])?,
            true,
        )?;
        Ok(json!(self.url()?))
    }
    fn create(&self, transport: &mut Transport<'_>, working: &Path) -> Result<Value> {
        let body = json!({"tag_name":self.tag()?,"target_commitish":self.revision()?,"name":format!("Swift Bonsai 2 Runner {}",self.tag()?),"body":self.policy["notes"],"draft":true,"prerelease":self.policy["prerelease"]});
        let input = working.join("release-create.json");
        write_json(&input, &body)?;
        let failure = transport(
            "gh",
            &[
                "api".into(),
                "--method".into(),
                "POST".into(),
                self.endpoint("releases")?,
                "--input".into(),
                input.to_string_lossy().into_owned(),
            ],
            180,
        )
        .err();
        let release = self.find(transport)?.ok_or_else(|| {
            format!(
                "Draft creation was not confirmed; mutation is not retried. {}",
                failure.unwrap_or_default()
            )
        })?;
        self.check(&release, true, 0, false)?;
        Ok(release)
    }
    fn upload(&self, transport: &mut Transport<'_>, id: u64) -> Result<()> {
        let assets = self.policy["assets"].as_object().ok_or("Missing assets.")?;
        for (name, asset) in assets {
            let release = self.get(transport, id)?;
            self.check(&release, true, id, false)?;
            if has_asset(&release, name) {
                continue;
            }
            let file = path(asset, "Path")?;
            if crate::startup_cache::digest_file(&file)? != text(asset, "Sha256")?
                || fs::metadata(&file).map_err(|e| e.to_string())?.len()
                    != asset["Bytes"].as_u64().ok_or("Invalid upload extent.")?
            {
                return Err("Release upload bytes changed.".into());
            }
            let failure = transport(
                "gh",
                &[
                    "api".into(),
                    "--method".into(),
                    "POST".into(),
                    format!(
                        "https://uploads.github.com/repos/{}/releases/{id}/assets?name={name}",
                        self.repository()?
                    ),
                    "--header".into(),
                    "Content-Type: application/octet-stream".into(),
                    "--input".into(),
                    file.to_string_lossy().into_owned(),
                ],
                600,
            )
            .err();
            let release = self.get(transport, id)?;
            self.check(&release, true, id, false)?;
            if !has_asset(&release, name) {
                return Err(format!(
                    "Upload was not confirmed for {name}; release remains draft. {}",
                    failure.unwrap_or_default()
                ));
            }
        }
        Ok(())
    }
    fn finish(&self, transport: &mut Transport<'_>, id: u64, working: &Path) -> Result<Value> {
        let release = self.get(transport, id)?;
        self.check(&release, true, id, true)?;
        self.confirm_tag(transport, false)?;
        self.main_matches(transport)?;
        let input = working.join("release-publish.json");
        write_json(
            &input,
            &json!({"draft":false,"prerelease":self.policy["prerelease"],"make_latest":if self.policy["prerelease"]==true { "false" } else { "true" }}),
        )?;
        let _failure = transport(
            "gh",
            &[
                "api".into(),
                "--method".into(),
                "PATCH".into(),
                self.endpoint(&format!("releases/{id}"))?,
                "--input".into(),
                input.to_string_lossy().into_owned(),
            ],
            180,
        )
        .err();
        self.check(&self.get(transport, id)?, false, id, true)?;
        self.confirm_tag(transport, true)?;
        Ok(json!(self.url()?))
    }
    fn execute(&self, transport: &mut Transport<'_>, working: &Path) -> Result<Value> {
        let release = self.find(transport)?;
        if let Some(release) = release
            .as_ref()
            .filter(|release| release["draft"].as_bool() == Some(false))
        {
            return self.existing(transport, release);
        }
        self.main_matches(transport)?;
        self.confirm_tag(transport, false)?;
        let root = self.repository_root.to_string_lossy().into_owned();
        let tag = self.tag()?.to_owned();
        if !transport(
            "git",
            &[
                "-C".into(),
                root.clone(),
                "tag".into(),
                "--list".into(),
                tag.clone(),
            ],
            180,
        )?
        .trim()
        .is_empty()
            && transport(
                "git",
                &[
                    "-C".into(),
                    root,
                    "rev-parse".into(),
                    format!("{tag}^{{commit}}"),
                ],
                180,
            )?
            .trim()
                != self.revision()?
        {
            return Err("Local release tag points to different source.".into());
        }
        let release = if let Some(release) = release {
            self.check(&release, true, 0, false)?;
            release
        } else {
            self.create(transport, working)?
        };
        let id = super::id(&release["id"])?;
        self.upload(transport, id)?;
        self.finish(transport, id, working)
    }
}

fn has_asset(release: &Value, name: &str) -> bool {
    release["assets"]
        .as_array()
        .is_some_and(|assets| assets.iter().any(|asset| asset["name"] == name))
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub(super) fn publish(value: &Value) -> Result<Value> {
    let repository = text(value, "repository")?;
    let revision = text(value, "sourceRevision")?;
    let tag = text(value, "tag")?;
    let prerelease = value["prerelease"]
        .as_bool()
        .ok_or("Missing release prerelease policy.")?;
    if repository != "symbiogenesis/SwiftBonsai2Runner"
        || std::env::var("GITHUB_REPOSITORY").as_deref() != Ok(repository)
        || std::env::var("GITHUB_REF").as_deref() != Ok("refs/heads/main")
        || std::env::var("GITHUB_SHA").as_deref() != Ok(revision)
    {
        return Err("Publishing requires repository main at dispatched source.".into());
    }
    let version = version(tag, revision)?;
    let root = resolve(&path(value, "repositoryRoot")?, true, false)?;
    resolve(&root.join("README.md"), false, false)?;
    let directory = resolve(&path(value, "assetDirectory")?, true, false)?;
    let mut identities = BTreeMap::new();
    let mut guards = Vec::new();
    for name in [
        format!("SwiftBonsai2Runner-{version}-windows-arm64.exe"),
        "release-manifest.json".into(),
        "SHA256SUMS.txt".into(),
    ] {
        let file = directory.join(&name);
        guards.push(crate::build_tools::input_file(&file)?);
        identities.insert(name,json!({"Path":file,"Bytes":fs::metadata(&file).map_err(|e| e.to_string())?.len(),"Sha256":crate::startup_cache::digest_file(&file)?}));
    }
    assets::validate(value)?;
    let publication = Publication {
        repository_root: root,
        policy: json!({"repository":repository,"tag":tag,"sourceRevision":revision,"notes":format!("See the [application guide](https://github.com/{repository}/blob/{revision}/README.md) for features, setup and qualification status."),"prerelease":prerelease,"assets":identities}),
    };
    let working = std::env::temp_dir().join(format!(
        "SwiftBonsai-Publish-{}",
        crate::startup_process::unique()
    ));
    fs::create_dir(&working).map_err(|e| e.to_string())?;
    let result = publication.execute(
        &mut |file, args, timeout| {
            let output = process::invoke(
                &json!({"filePath":file,"arguments":args,"timeoutSeconds":timeout}),
            )?;
            Ok(text(&output, "stdout")?.into())
        },
        &working,
    );
    let cleanup = process::cleanup(
        &json!({"path":working,"parentDirectory":std::env::temp_dir(),"leafName":working.file_name().and_then(|name| name.to_str()).ok_or("Missing publication leaf.")?}),
    );
    drop(guards);
    match (result, cleanup) {
        (Ok(value), Ok(_)) => Ok(value),
        (Err(error), Ok(_)) | (Ok(_), Err(error)) => Err(error),
        (Err(primary), Err(cleanup)) => {
            Err(format!("{primary}\nRelease cleanup also failed: {cleanup}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_publication_ambiguous_mutations_are_read_back_once_and_published_replay_never_writes()
    -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "release-publish-test-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|e| e.to_string())?;
        let file = root.join("sample.exe");
        fs::write(&file, b"inert").map_err(|e| e.to_string())?;
        let revision = "a".repeat(40);
        let digest = crate::identity::sha256(b"inert");
        let policy = json!({"repository":"example/repo","tag":"v1.0.0","sourceRevision":revision,"notes":"reviewed notes","prerelease":true,"assets":{"sample.exe":{"Path":file,"Bytes":5,"Sha256":digest}}});
        let publication = Publication {
            repository_root: root.clone(),
            policy,
        };
        let mut release = Value::Null;
        let mut mutations = Vec::new();
        let asset = json!({"id":2,"name":"sample.exe","state":"uploaded","size":5,"digest":format!("sha256:{digest}")});
        publication.execute(&mut |command,args,_| {
            if command=="git" { return Ok(String::new()); }
            let method=if args.get(1).is_some_and(|s| s=="--method") { args[2].as_str() } else { "GET" };
            let endpoint=if method=="GET" { &args[1] } else { &args[3] };
            if method!="GET" {
                mutations.push(method.to_owned());
                if endpoint.ends_with("/releases") { release=json!({"id":1,"tag_name":"v1.0.0","target_commitish":revision,"name":"Swift Bonsai 2 Runner v1.0.0","html_url":"https://github.com/example/repo/releases/tag/v1.0.0","body":"reviewed notes","draft":true,"prerelease":true,"assets":[]}); }
                else if method=="POST" { release["assets"]=json!([asset.clone()]); }
                else { release["draft"]=false.into(); }
                return Err("Lost mutation response".into());
            }
            if endpoint.ends_with("commits/main") { return Ok(revision.clone()); }
            if endpoint.contains("matching-refs") { return Ok(if release["draft"]==false { json!([{"ref":"refs/tags/v1.0.0","object":{"type":"commit","sha":revision}}]).to_string() } else { "[]".into() }); }
            if endpoint.ends_with("?per_page=100") { return Ok(if release.is_null() { String::new() } else { json!({"id":1,"tag_name":"v1.0.0"}).to_string() }); }
            Ok(release.to_string())
        },&root)?;
        assert_eq!(mutations, ["POST", "POST", "PATCH"]);
        publication.execute(&mut |command,args,_| {
            assert_eq!(command,"gh"); assert_ne!(args[1],"--method"); assert!(!args[1].contains("commits/main"));
            if args[1].contains("matching-refs") { return Ok(json!([{"ref":"refs/tags/v1.0.0","object":{"type":"commit","sha":revision}}]).to_string()); }
            if args[1].ends_with("?per_page=100") { return Ok(json!({"id":1,"tag_name":"v1.0.0"}).to_string()); }
            Ok(release.to_string())
        },&root)?;
        fs::remove_file(file)
            .and_then(|()| fs::remove_file(root.join("release-create.json")))
            .and_then(|()| fs::remove_file(root.join("release-publish.json")))
            .and_then(|()| fs::remove_dir(root))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    #[test]
    fn release_publication_unconfirmed_mutations_and_advanced_main_never_retry_or_publish()
    -> Result<()> {
        for stage in ["create", "upload", "publish", "readback", "advance"] {
            rejected_stage(stage)?;
        }
        Ok(())
    }

    fn rejected_stage(stage: &str) -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "release-reject-test-{}",
            crate::startup_process::unique()
        ));
        fs::create_dir(&root).map_err(|e| e.to_string())?;
        let file = root.join("sample.exe");
        fs::write(&file, b"inert").map_err(|e| e.to_string())?;
        let revision = "a".repeat(40);
        let digest = crate::identity::sha256(b"inert");
        let asset = json!({"id":2,"name":"sample.exe","state":"uploaded","size":5,"digest":format!("sha256:{digest}")});
        let publication = Publication {
            repository_root: root.clone(),
            policy: json!({"repository":"example/repo","tag":"v1.0.0","sourceRevision":revision,"notes":"reviewed notes","prerelease":true,"assets":{"sample.exe":{"Path":file,"Bytes":5,"Sha256":digest}}}),
        };
        let release = if stage == "create" {
            Value::Null
        } else {
            json!({"id":1,"tag_name":"v1.0.0","target_commitish":revision,"name":"Swift Bonsai 2 Runner v1.0.0","html_url":"https://github.com/example/repo/releases/tag/v1.0.0","body":"reviewed notes","draft":true,"prerelease":true,"assets":if ["publish","advance"].contains(&stage) { vec![asset] } else { vec![] }})
        };
        let mut mutations = Vec::new();
        let mut unreadable = false;
        let mut main_reads = 0;
        let result = publication.execute(
            &mut |command, args, _| {
                if command == "git" {
                    return Ok(String::new());
                }
                if args[1] == "--method" {
                    mutations.push(args[2].clone());
                    unreadable = stage == "readback";
                    return Err("Unconfirmed mutation".into());
                }
                if args[1].ends_with("commits/main") {
                    main_reads += 1;
                    return Ok(if stage == "advance" && main_reads > 1 {
                        "b".repeat(40)
                    } else {
                        revision.clone()
                    });
                }
                if args[1].contains("matching-refs") {
                    return Ok("[]".into());
                }
                if args[1].ends_with("?per_page=100") {
                    return Ok(if release.is_null() {
                        String::new()
                    } else {
                        json!({"id":1,"tag_name":"v1.0.0"}).to_string()
                    });
                }
                if unreadable {
                    return Err("Authoritative readback unavailable".into());
                }
                Ok(release.to_string())
            },
            &root,
        );
        assert!(result.is_err(), "Accepted incomplete {stage}");
        let expected = match stage {
            "publish" => vec!["PATCH"],
            "advance" => vec![],
            _ => vec!["POST"],
        };
        assert_eq!(mutations, expected);
        fs::remove_file(file).map_err(|e| e.to_string())?;
        for name in ["release-create.json", "release-publish.json"] {
            let path = root.join(name);
            if path.exists() {
                fs::remove_file(path).map_err(|e| e.to_string())?;
            }
        }
        fs::remove_dir(root).map_err(|e| e.to_string())?;
        Ok(())
    }
}
