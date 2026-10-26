//! The GitHub operations of a benchmark: scenario commits on throwaway branches (Git Data API,
//! no clone), workflow dispatches, run polling, cancellation and cleanup.
//!
//! | Operation | Endpoint |
//! |---|---|
//! | [`head`] | `GET /repos/{repo}/git/ref/heads/{branch}` |
//! | [`read_file`] | `GET /repos/{repo}/contents/{path}?ref={sha}` (404: absent) |
//! | [`commit_branch`] | `GET …/git/commits/{base}`, `POST …/git/trees`, `POST …/git/commits`, `POST …/git/refs` |
//! | [`delete_branch`] | `DELETE …/git/refs/heads/{branch}` (422/404: already gone) |
//! | [`dispatch`] | `POST …/actions/workflows/{file}/dispatches` (200 names the run; 204: find it by tag) |
//! | [`find_run`] | `GET …/actions/workflows/{file}/runs?branch=…&event=workflow_dispatch` |
//! | [`run`], [`job`] | `GET …/actions/runs/{id}`, `GET …/actions/runs/{id}/jobs` |
//! | [`cancel`] | `POST …/actions/runs/{id}/cancel` |

use base64::Engine as _;
use serde_json::{Value, json};

use crate::github::{Api, ApiError, Client, Method};
use crate::scenario::Change;

fn bad(method: Method, path: &str, detail: impl Into<String>) -> ApiError {
    ApiError {
        method: method.as_str(),
        path: path.to_owned(),
        detail: detail.into(),
    }
}

fn sha_of(v: &Value, pointer: &str, method: Method, path: &str) -> Result<String, ApiError> {
    v.pointer(pointer)
        .and_then(Value::as_str)
        .filter(|s| s.len() == 40)
        .map(str::to_owned)
        .ok_or_else(|| bad(method, path, format!("no {pointer} in the answer")))
}

/// The commit `branch` points at.
///
/// # Errors
/// [`ApiError`].
pub async fn head(client: &Client<impl Api>, repo: &str, branch: &str) -> Result<String, ApiError> {
    let path = format!("/repos/{repo}/git/ref/heads/{branch}");
    let v = client.ok(Method::Get, &path, None).await?;
    sha_of(&v, "/object/sha", Method::Get, &path)
}

/// A file's bytes at `sha`, `None` if it does not exist (files up to the contents API's 1 MB).
///
/// # Errors
/// [`ApiError`] (a directory or a file too large for the contents API included).
pub async fn read_file(
    client: &Client<impl Api>,
    repo: &str,
    sha: &str,
    path: &str,
) -> Result<Option<Vec<u8>>, ApiError> {
    let url = format!("/repos/{repo}/contents/{path}?ref={sha}");
    let r = client.call(Method::Get, &url, None).await?;
    if r.status == 404 {
        return Ok(None);
    }
    if !r.ok() {
        return Err(bad(
            Method::Get,
            &url,
            format!("answered {}: {}", r.status, r.message()),
        ));
    }
    if r.json["type"] != "file" || r.json["encoding"] != "base64" {
        return Err(bad(Method::Get, &url, "not a file of at most 1 MB"));
    }
    let text: String = r.json["content"]
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(text)
        .map(Some)
        .map_err(|e| bad(Method::Get, &url, e.to_string()))
}

/// Commits `changes` (paths under `prefix/`) on top of `base` and points a new `branch` at
/// it. Written files are regular (`100644`) UTF-8 files.
///
/// # Errors
/// [`ApiError`]; a non-UTF-8 change is refused before anything is written.
pub async fn commit_branch(
    client: &Client<impl Api>,
    repo: &str,
    base: &str,
    prefix: &str,
    changes: &[Change],
    branch: &str,
    message: &str,
) -> Result<String, ApiError> {
    let trees = format!("/repos/{repo}/git/trees");
    let entries = changes
        .iter()
        .map(|c| {
            let path = format!("{prefix}/{}", c.path());
            match c {
                Change::Write { content, .. } => {
                    let text = std::str::from_utf8(content)
                        .map_err(|_| bad(Method::Post, &trees, format!("{path} is not UTF-8")))?;
                    Ok(json!({"path": path, "mode": "100644", "type": "blob", "content": text}))
                }
                Change::Remove { .. } => {
                    Ok(json!({"path": path, "mode": "100644", "type": "blob", "sha": null}))
                }
            }
        })
        .collect::<Result<Vec<Value>, ApiError>>()?;
    let commit_path = format!("/repos/{repo}/git/commits/{base}");
    let base_commit = client.ok(Method::Get, &commit_path, None).await?;
    let base_tree = sha_of(&base_commit, "/tree/sha", Method::Get, &commit_path)?;
    let tree = client
        .ok(
            Method::Post,
            &trees,
            Some(&json!({"base_tree": base_tree, "tree": entries})),
        )
        .await?;
    let tree = sha_of(&tree, "/sha", Method::Post, &trees)?;
    let commits = format!("/repos/{repo}/git/commits");
    let commit = client
        .ok(
            Method::Post,
            &commits,
            Some(&json!({"message": message, "tree": tree, "parents": [base]})),
        )
        .await?;
    let commit = sha_of(&commit, "/sha", Method::Post, &commits)?;
    let refs = format!("/repos/{repo}/git/refs");
    client
        .ok(
            Method::Post,
            &refs,
            Some(&json!({"ref": format!("refs/heads/{branch}"), "sha": commit})),
        )
        .await?;
    Ok(commit)
}

/// Deletes `branch`; one that is already gone is fine.
///
/// # Errors
/// [`ApiError`] for any other failure.
pub async fn delete_branch(
    client: &Client<impl Api>,
    repo: &str,
    branch: &str,
) -> Result<(), ApiError> {
    let path = format!("/repos/{repo}/git/refs/heads/{branch}");
    let r = client.call(Method::Delete, &path, None).await?;
    match r.status {
        204 | 404 | 422 => Ok(()),
        s => Err(bad(
            Method::Delete,
            &path,
            format!("answered {s}: {}", r.message()),
        )),
    }
}

/// Dispatches `workflow` on `git_ref` with `inputs`: the run's id when GitHub names it.
///
/// # Errors
/// [`ApiError`] (an unknown workflow or ref, or invalid inputs, are 4xx).
pub async fn dispatch(
    client: &Client<impl Api>,
    repo: &str,
    workflow: &str,
    git_ref: &str,
    inputs: &Value,
) -> Result<Option<u64>, ApiError> {
    let path = format!("/repos/{repo}/actions/workflows/{workflow}/dispatches");
    let r = client
        .call(
            Method::Post,
            &path,
            Some(&json!({"ref": git_ref, "inputs": inputs})),
        )
        .await?;
    match r.status {
        200 => Ok(r.json["workflow_run_id"].as_u64()),
        204 => Ok(None),
        s => Err(bad(
            Method::Post,
            &path,
            format!("answered {s}: {}", r.message()),
        )),
    }
}

/// The newest dispatched run of `workflow` on `branch` whose title ends with `tag` (the
/// workflows' `run-name` ends with the tag input).
///
/// # Errors
/// [`ApiError`].
pub async fn find_run(
    client: &Client<impl Api>,
    repo: &str,
    workflow: &str,
    branch: &str,
    tag: &str,
) -> Result<Option<u64>, ApiError> {
    let path = format!(
        "/repos/{repo}/actions/workflows/{workflow}/runs?branch={branch}&event=workflow_dispatch&per_page=50"
    );
    let v = client.ok(Method::Get, &path, None).await?;
    let suffix = format!(" {tag}");
    Ok(v["workflow_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| {
            r["display_title"]
                .as_str()
                .is_some_and(|t| t.ends_with(&suffix))
        })
        .filter_map(|r| r["id"].as_u64())
        .max())
}

/// A run's JSON.
///
/// # Errors
/// [`ApiError`].
pub async fn run(client: &Client<impl Api>, repo: &str, id: u64) -> Result<Value, ApiError> {
    client
        .ok(
            Method::Get,
            &format!("/repos/{repo}/actions/runs/{id}"),
            None,
        )
        .await
}

/// The run's job (the workflows have one), latest attempt.
///
/// # Errors
/// [`ApiError`], or not exactly one job.
pub async fn job(client: &Client<impl Api>, repo: &str, id: u64) -> Result<Value, ApiError> {
    let path = format!("/repos/{repo}/actions/runs/{id}/jobs?filter=latest");
    let v = client.ok(Method::Get, &path, None).await?;
    match v["jobs"].as_array().map(Vec::as_slice) {
        Some([job]) => Ok(job.clone()),
        other => Err(bad(
            Method::Get,
            &path,
            format!(
                "expected one job, found {}",
                other.map_or(0, <[Value]>::len)
            ),
        )),
    }
}

/// Asks GitHub to cancel a run (`409`: it already finished).
///
/// # Errors
/// [`ApiError`].
pub async fn cancel(client: &Client<impl Api>, repo: &str, id: u64) -> Result<(), ApiError> {
    let path = format!("/repos/{repo}/actions/runs/{id}/cancel");
    let r = client.call(Method::Post, &path, None).await?;
    match r.status {
        202 | 409 => Ok(()),
        s => Err(bad(
            Method::Post,
            &path,
            format!("answered {s}: {}", r.message()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::tests::Queue;
    use crate::github::{Limits, Response};

    fn client(answers: Vec<Response>) -> Client<Queue> {
        let queue = Queue::default();
        queue
            .answers
            .borrow_mut()
            .extend(answers.into_iter().map(Ok));
        Client::new(queue, Limits::STANDARD)
    }

    fn sha(c: char) -> String {
        c.to_string().repeat(40)
    }

    #[tokio::test]
    async fn branch_commits_and_cleanup() {
        let c = client(vec![
            Response::new(200, json!({"tree": {"sha": sha('t')}})),
            Response::new(201, json!({"sha": sha('u')})),
            Response::new(201, json!({"sha": sha('c')})),
            Response::new(201, json!({"ref": "refs/heads/bench/x"})),
            Response::new(422, json!({"message": "Reference does not exist"})),
            Response::new(500, Value::Null),
        ]);
        let changes = [
            Change::Write {
                path: "a.go".parse().unwrap(),
                content: b"package a\n".to_vec(),
            },
            Change::Remove {
                path: "b_test.go".parse().unwrap(),
            },
        ];
        let commit = commit_branch(
            &c,
            "o/r",
            &sha('b'),
            "fixtures/go-small",
            &changes,
            "bench/x",
            "m",
        )
        .await
        .unwrap();
        assert_eq!(commit, sha('c'));
        let calls = c_calls(&c);
        assert_eq!(calls[1].1, "/repos/o/r/git/trees");
        let tree = calls[1].2.as_ref().unwrap();
        assert_eq!(tree["base_tree"], sha('t'));
        assert_eq!(tree["tree"][0]["path"], "fixtures/go-small/a.go");
        assert_eq!(tree["tree"][0]["content"], "package a\n");
        assert_eq!(tree["tree"][1]["sha"], Value::Null);
        assert_eq!(calls[2].2.as_ref().unwrap()["parents"][0], sha('b'));
        assert_eq!(calls[3].2.as_ref().unwrap()["ref"], "refs/heads/bench/x");
        delete_branch(&c, "o/r", "bench/x").await.unwrap();
        let limited = Client::new(
            c.into_api(),
            Limits {
                attempts: 1,
                ..Limits::STANDARD
            },
        );
        assert!(delete_branch(&limited, "o/r", "bench/x").await.is_err());
        let binary = [Change::Write {
            path: "x".parse().unwrap(),
            content: vec![0xff],
        }];
        let none = client(vec![]);
        assert!(
            commit_branch(&none, "o/r", &sha('b'), "f", &binary, "b", "m")
                .await
                .is_err()
        );
    }

    fn c_calls(c: &Client<Queue>) -> Vec<(Method, String, Option<Value>)> {
        c.api_ref().calls.borrow().clone()
    }

    #[tokio::test]
    async fn files_runs_and_dispatches() {
        let content = base64::engine::general_purpose::STANDARD.encode("hello\n");
        let c = client(vec![
            Response::new(
                200,
                json!({"type": "file", "encoding": "base64", "content": format!("{}\n{}", &content[..4], &content[4..])}),
            ),
            Response::new(404, json!({"message": "Not Found"})),
            Response::new(200, json!({"type": "dir"})),
            Response::new(
                200,
                json!({"workflow_run_id": 77, "run_url": "u", "html_url": "h"}),
            ),
            Response::new(204, Value::Null),
            Response::new(422, json!({"message": "Unexpected inputs"})),
            Response::new(
                200,
                json!({"workflow_runs": [
                {"id": 5, "display_title": "go-small leaf t-1"},
                {"id": 9, "display_title": "go-small leaf t-12"},
                {"id": 7, "display_title": "go-small leaf t-1"}]}),
            ),
            Response::new(200, json!({"jobs": [{"id": 1}]})),
            Response::new(200, json!({"jobs": []})),
            Response::new(
                409,
                json!({"message": "Cannot cancel a workflow run that is completed."}),
            ),
            Response::new(200, json!({"object": {"sha": sha('h')}})),
        ]);
        assert_eq!(
            read_file(&c, "o/r", &sha('b'), "a").await.unwrap(),
            Some(b"hello\n".to_vec())
        );
        assert_eq!(read_file(&c, "o/r", &sha('b'), "b").await.unwrap(), None);
        assert!(read_file(&c, "o/r", &sha('b'), "d").await.is_err());
        let inputs = json!({"fixture": "go-small"});
        assert_eq!(
            dispatch(&c, "o/r", "w.yml", "main", &inputs).await.unwrap(),
            Some(77)
        );
        assert_eq!(
            dispatch(&c, "o/r", "w.yml", "main", &inputs).await.unwrap(),
            None
        );
        assert!(dispatch(&c, "o/r", "w.yml", "main", &inputs).await.is_err());
        assert_eq!(
            find_run(&c, "o/r", "w.yml", "b", "t-1").await.unwrap(),
            Some(7)
        );
        assert_eq!(job(&c, "o/r", 7).await.unwrap()["id"], 1);
        assert!(job(&c, "o/r", 7).await.is_err());
        cancel(&c, "o/r", 7).await.unwrap();
        assert_eq!(head(&c, "o/r", "main").await.unwrap(), sha('h'));
        let calls = c_calls(&c);
        assert_eq!(
            calls[0].1,
            format!("/repos/o/r/contents/a?ref={}", sha('b'))
        );
        assert_eq!(calls[3].2.as_ref().unwrap()["ref"], "main");
        assert!(calls[6].1.contains("event=workflow_dispatch"));
        assert_eq!(calls[7].1, "/repos/o/r/actions/runs/7/jobs?filter=latest");
    }
}
