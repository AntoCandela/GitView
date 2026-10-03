//! Owns disposable Git history and the only filesystem mutations allowed by the journey bridge.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tempfile::TempDir;

pub const MAX_EDIT_BYTES: usize = 1024 * 1024;
const WORKING_PATH: &str = "src/working.txt";
const UNTRACKED_PATH: &str = "notes/untracked.txt";
const UNCHANGED_PATH: &str = "src/unchanged.txt";
const HEAD_TEXT: &str = "head version\n";
const INDEX_TEXT: &str = "index version\n";
const WORKING_TEXT: &str = "working version\n";
const UNTRACKED_TEXT: &str = "untracked note\n";
const UNCHANGED_TEXT: &str = "unchanged reference\n";
const ORGANIZATION_PATH: &str = "src/organization.ts";
const ORGANIZATION_HEAD: &str = "export const version: string = \"committed organization\";\n";
const ORGANIZATION_INDEX: &str = "export const version: string = \"staged organization\";\n";
const ORGANIZATION_WORKING: &str = "export const version: string = \"working organization\";\nexport const inert: string = '<img src=x onerror=\"globalThis.journeyInjected=true\">';\n";
const ROOT_ENTRY_COUNT: usize = 768;
const SHADER_FILES: [(&str, &str); 2] = [
    ("src/shader.cpp", "const char* shader = R\"glsl(\n// shader comment\nuniform vec4 tint;\nvoid main() { gl_FragColor = tint; }\n)glsl\";\n"),
    ("src/shader.rb", "shader = <<~CPP\nconst char* shader = R\"glsl(\n// shader comment\nuniform vec4 tint;\nvoid main() { gl_FragColor = tint; }\n)glsl\";\nCPP\n"),
];

pub struct JourneyFixture {
    owner: TempDir,
    pub main: PathBuf,
    pub other: PathBuf,
    linked: PathBuf,
    hidden: PathBuf,
    baseline: Vec<Vec<u8>>,
    expected_working: String,
    organization: bool,
}

impl JourneyFixture {
    pub fn create(organization: bool) -> Result<Self, &'static str> {
        let owner = tempfile::tempdir().map_err(|_| "Fixture directory failed.")?;
        let main = owner.path().join("journey-main");
        let other = owner.path().join("journey-other");
        let linked = owner.path().join("journey-linked");
        let hidden = owner.path().join("hidden-main");
        for root in [&main, &other] {
            fs::create_dir(root).map_err(|_| "Fixture directory failed.")?;
            git(root, &["init", "--template=", "-b", "main"])?;
            import_history(root, organization)?;
            git(root, &["reset", "--hard", "main"])?;
        }
        git(&main, &["worktree", "add", "-b", "journey-linked", path_text(&linked)?, "topic"])?;
        fs::write(main.join(WORKING_PATH), INDEX_TEXT).map_err(|_| "Fixture write failed.")?;
        git(&main, &["add", "--", WORKING_PATH])?;
        fs::write(main.join(WORKING_PATH), WORKING_TEXT).map_err(|_| "Fixture write failed.")?;
        fs::create_dir(main.join("notes")).map_err(|_| "Fixture directory failed.")?;
        fs::write(main.join(UNTRACKED_PATH), UNTRACKED_TEXT).map_err(|_| "Fixture write failed.")?;
        if organization {
            fs::write(main.join(ORGANIZATION_PATH), ORGANIZATION_INDEX).map_err(|_| "Fixture write failed.")?;
            git(&main, &["add", "--", ORGANIZATION_PATH])?;
            fs::write(main.join(ORGANIZATION_PATH), ORGANIZATION_WORKING).map_err(|_| "Fixture write failed.")?;
            for (path, content) in SHADER_FILES {
                fs::write(main.join(path), content).map_err(|_| "Fixture write failed.")?;
            }
            for index in 0..ROOT_ENTRY_COUNT {
                fs::write(main.join(root_entry_path(index)), root_entry_text(index)).map_err(|_| "Fixture write failed.")?;
            }
        }
        let mut fixture = Self {
            owner, main, other, linked, hidden, baseline: Vec::new(),
            expected_working: WORKING_TEXT.to_owned(), organization,
        };
        fixture.baseline = fixture.git_state()?;
        Ok(fixture)
    }

    pub fn workspace_file(&self) -> PathBuf {
        self.owner.path().join("workspace.json")
    }

    pub fn info(&self) -> Value {
        let mut info = json!({
            "mainLabel": "journey-main", "otherLabel": "journey-other",
            "mainBranch": "main", "topicBranch": "topic",
            "workingPath": WORKING_PATH, "untrackedPath": UNTRACKED_PATH,
            "unchangedPath": UNCHANGED_PATH, "headText": HEAD_TEXT,
            "indexText": INDEX_TEXT, "workingText": WORKING_TEXT,
            "untrackedText": UNTRACKED_TEXT, "unchangedText": UNCHANGED_TEXT,
            "mergeSubject": "Journey merge", "rootSubject": "Journey root",
        });
        if self.organization {
            info["organization"] = json!({
                "sourcePath": ORGANIZATION_PATH,
                "headText": ORGANIZATION_HEAD,
                "indexText": ORGANIZATION_INDEX,
                "workingText": ORGANIZATION_WORKING,
                "latePath": root_entry_path(ROOT_ENTRY_COUNT - 1),
                "lateText": root_entry_text(ROOT_ENTRY_COUNT - 1),
                "rootEntryCount": ROOT_ENTRY_COUNT,
                "shaders": SHADER_FILES.map(|(path, content)| json!({ "path": path, "content": content })),
            });
        }
        info
    }

    pub fn edit(&mut self, text: &str) -> Result<(), &'static str> {
        if text.len() > MAX_EDIT_BYTES { return Err("Fixture text exceeds limit."); }
        fs::write(self.main.join(WORKING_PATH), text).map_err(|_| "Fixture write failed.")?;
        self.expected_working = text.to_owned();
        Ok(())
    }

    pub fn hide(&self) -> Result<(), &'static str> {
        if self.hidden.exists() { return Err("Fixture already hidden."); }
        fs::rename(&self.main, &self.hidden).map_err(|_| "Fixture hide failed.")
    }

    pub fn restore(&self) -> Result<(), &'static str> {
        if self.main.exists() { return Err("Fixture is not hidden."); }
        fs::rename(&self.hidden, &self.main).map_err(|_| "Fixture restore failed.")
    }

    pub fn verify(&self) -> Result<Value, &'static str> {
        let repositories_intact = self.main.join(".git").is_dir()
            && self.other.join(".git").is_dir() && self.linked.join(".git").is_file();
        let git_state_unchanged = repositories_intact && self.git_state()? == self.baseline;
        let working_bytes_expected = fs::read(self.main.join(WORKING_PATH)).ok().as_deref()
            == Some(self.expected_working.as_bytes())
            && fs::read(self.main.join(UNTRACKED_PATH)).ok().as_deref() == Some(UNTRACKED_TEXT.as_bytes())
            && fs::read(self.main.join(UNCHANGED_PATH)).ok().as_deref() == Some(UNCHANGED_TEXT.as_bytes())
            && self.organization_bytes_expected();
        Ok(json!({
            "repositoriesIntact": repositories_intact,
            "gitStateUnchanged": git_state_unchanged,
            "workingBytesExpected": working_bytes_expected,
        }))
    }

    fn organization_bytes_expected(&self) -> bool {
        if !self.organization { return true; }
        fs::read(self.main.join(ORGANIZATION_PATH)).ok().as_deref() == Some(ORGANIZATION_WORKING.as_bytes())
            && [&self.other, &self.linked].iter().all(|root| {
                fs::read(root.join(ORGANIZATION_PATH)).ok().as_deref() == Some(ORGANIZATION_HEAD.as_bytes())
            })
            && SHADER_FILES.iter().all(|(path, content)| {
                fs::read(self.main.join(path)).ok().as_deref() == Some(content.as_bytes())
            })
            && (0..ROOT_ENTRY_COUNT).all(|index| {
                fs::read(self.main.join(root_entry_path(index))).ok().as_deref()
                    == Some(root_entry_text(index).as_bytes())
            })
    }

    fn git_state(&self) -> Result<Vec<Vec<u8>>, &'static str> {
        let mut state = Vec::new();
        for root in [&self.main, &self.other, &self.linked] {
            state.push(git(root, &["rev-parse", "HEAD"])?);
            state.push(git(root, &["symbolic-ref", "HEAD"])?);
            state.push(git(root, &["show-ref"])?);
            let index = git(root, &["rev-parse", "--git-path", "index"])?;
            let index = std::str::from_utf8(&index).map_err(|_| "Fixture index path failed.")?.trim();
            state.push(fs::read(root.join(index)).map_err(|_| "Fixture index read failed.")?);
            let diff_args: &[&str] = if root == &self.main {
                &["diff", "--no-ext-diff", "--binary", "--", ".", ":(exclude)src/working.txt"]
            } else {
                &["diff", "--no-ext-diff", "--binary", "--", "."]
            };
            state.push(git(root, diff_args)?);
        }
        Ok(state)
    }
}

fn root_entry_path(index: usize) -> String {
    format!("entry-{index:04}.ts")
}

fn root_entry_text(index: usize) -> String {
    format!("export const entry{index:04}: number = {index};\n")
}

fn path_text(path: &Path) -> Result<&str, &'static str> {
    path.to_str().ok_or("Fixture path encoding failed.")
}

fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(root);
    // Per-child isolation also removes injected numbered config and alternate object stores.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") { command.env_remove(key); }
    }
    command.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", root.join(".gitview-empty-global-config"))
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["-c", "core.hooksPath=", "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"]);
    command
}

fn git(root: &Path, arguments: &[&str]) -> Result<Vec<u8>, &'static str> {
    let output = git_command(root).args(arguments).output().map_err(|_| "Fixture Git launch failed.")?;
    if !output.status.success() { return Err("Fixture Git operation failed."); }
    Ok(output.stdout)
}

fn import_history(root: &Path, organization: bool) -> Result<(), &'static str> {
    let mut stream = String::new();
    commit_record(&mut stream, "main", 1, "Journey root", None, None);
    inline_file(&mut stream, WORKING_PATH, HEAD_TEXT);
    inline_file(&mut stream, UNCHANGED_PATH, UNCHANGED_TEXT);
    if organization { inline_file(&mut stream, ORGANIZATION_PATH, ORGANIZATION_HEAD); }
    stream.push('\n');
    // Fast-import creates a real parent chain without one subprocess per commit.
    for mark in 2..=106 {
        commit_record(&mut stream, "main", mark, &format!("Journey history {mark:03}"), Some(mark - 1), None);
        stream.push('\n');
    }
    commit_record(&mut stream, "topic", 107, "Journey topic", Some(106), None);
    inline_file(&mut stream, "src/topic.txt", "topic contribution\n");
    stream.push('\n');
    commit_record(&mut stream, "main", 108, "Journey main change", Some(106), None);
    inline_file(&mut stream, "src/main.txt", "main contribution\n");
    stream.push('\n');
    commit_record(&mut stream, "main", 109, "Journey merge", Some(108), Some(107));
    inline_file(&mut stream, "src/topic.txt", "topic contribution\n");
    stream.push_str("\ndone\n");
    let mut child = git_command(root).args(["fast-import", "--quiet", "--done"])
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().map_err(|_| "Fixture import launch failed.")?;
    let written = child.stdin.take().ok_or("Fixture import input failed.")?
        .write_all(stream.as_bytes());
    if written.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Fixture import input failed.");
    }
    if !child.wait().map_err(|_| "Fixture import wait failed.")?.success() {
        return Err("Fixture import failed.");
    }
    Ok(())
}

fn commit_record(stream: &mut String, branch: &str, mark: u32, subject: &str, parent: Option<u32>, merge: Option<u32>) {
    use std::fmt::Write;
    writeln!(stream, "commit refs/heads/{branch}\nmark :{mark}\ncommitter Journey <journey@example.invalid> {} +0000\ndata {}\n{subject}", 1_700_000_000 + mark, subject.len()).unwrap();
    if let Some(parent) = parent { writeln!(stream, "from :{parent}").unwrap(); }
    if let Some(merge) = merge { writeln!(stream, "merge :{merge}").unwrap(); }
}

fn inline_file(stream: &mut String, path: &str, content: &str) {
    use std::fmt::Write;
    writeln!(stream, "M 100644 inline {path}\ndata {}\n{content}", content.len()).unwrap();
}
