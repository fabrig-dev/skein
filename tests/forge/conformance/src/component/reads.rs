use super::{Hosted, Operation};
use crate::json;
use crate::{
    Result,
    facts::Fact,
    fixture::{Fixture, R},
};
use skein_forge::api::{self, Answer, Read};
use skein_lib::Time;
use std::path::Path;
const JOB_END: &str = "component-read-job-answers-the-failed-steps-end";
pub(super) fn bytes(text: &str) -> Box<[u8]> {
    text.as_bytes().into()
}
pub(super) fn commit(text: &str) -> Result<api::Commit> {
    let mut result = [0; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        result[index] = u8::from_str_radix(std::str::from_utf8(pair)?, 16)?;
    }
    Ok(result)
}
pub(super) fn repository(f: &Fixture) -> Result<api::Repository> {
    Ok(api::Repository { forge: 1, repository: f.repository.try_into()? })
}
pub(super) fn run(runner: Option<&Path>, f: &Fixture, h: &mut Hosted, facts: &mut Vec<Fact>) -> Result<()> {
    let issue = f
        .api("POST", &format!("{R}/issues"), &json::object(&[("body", &"read body"), ("title", &"read fixture")])?)?
        .root()
        .get("number")
        .u64()
        .ok_or("issue number")?;
    let comment = f
        .api("POST", &format!("{R}/issues/{issue}/comments"), &json::object(&[("body", &"thread fixture")])?)?
        .root()
        .get("id")
        .u64()
        .ok_or("comment id")?;
    let review = f
        .api(
            "POST",
            &format!("{R}/pulls/{}/reviews", f.feature),
            &crate::fixture::remark_review("review fixture", "remark fixture")?,
        )?
        .root()
        .get("id")
        .u64()
        .ok_or("review id")?;
    f.api(
        "POST",
        &format!("{R}/statuses/{}", f.head),
        &json::object(&[("context", &"fixture/test"), ("state", &"success")])?,
    )?;
    let repo = repository(f)?;
    let head = commit(&f.head)?;
    let base = commit(&f.main)?;
    let oldest = f
        .get(&format!("{R}/issues?state=all&sort=leastupdate&page=1&limit=1"))?
        .root()
        .nth(0)
        .get("number")
        .u64()
        .ok_or("least updated item")?;
    facts.push(h.observe("component-read-items", repo, Read::Items { since: Time::ZERO, page: 1, kind: None }.operation(), |a| matches!(a, Answer::Items { items, now, .. } if items.first().is_some_and(|i| i.number == oldest && i.kind == api::Kind::Pull) && *now > Time::ZERO)));
    facts.push(h.observe("component-read-item", repo, Read::Item { number: issue, after: 0 }.operation(), |a| matches!(a, Answer::Item { item, comments, more } if item.number == issue && item.title.as_ref() == b"read fixture" && item.body.as_ref() == b"read body" && comments.len() == 1 && comments[0].id == comment && comments[0].body.as_ref() == b"thread fixture" && !more)));
    for (name, read) in [
        ("component-read-pull", Read::Pull { number: f.feature }),
        ("component-read-pull-for", Read::PullFor { head: bytes("feature/slash"), base: bytes("main") }),
    ] {
        facts.push(h.observe(name, repo, read.operation(), |a| matches!(a, Answer::Pull(p) if p.number == f.feature && p.commit == head && p.head.as_ref() == b"feature/slash" && p.base.as_ref() == b"main" && p.state == api::State::Open)));
    }
    facts.push(h.observe("component-read-reviews", repo, Read::Reviews { number: f.feature, page: 1 }.operation(), |a| matches!(a, Answer::Reviews { reviews, .. } if reviews.len() == 1 && reviews[0].id == review && reviews[0].body.as_ref() == b"review fixture" && reviews[0].verdict == api::Verdict::Comment)));
    facts.push(h.observe("component-read-statuses", repo, Read::Statuses { commit: head, page: 1 }.operation(), |a| matches!(a, Answer::Statuses { ci: api::Ci::Passed, statuses, more: false } if statuses.len() == 1 && statuses[0].context.as_ref() == b"fixture/test" && statuses[0].check == api::Check::Passed)));
    facts.push(h.observe("component-read-remarks", repo, Read::Remarks { number: f.feature, review, page: 1 }.operation(), |a| matches!(a, Answer::Remarks { remarks, more: false } if remarks.len() == 1 && remarks[0].path.as_ref() == b"file-0.txt" && remarks[0].line == 1 && remarks[0].body.as_ref() == b"remark fixture")));
    facts.push(h.observe(
        "component-read-branch",
        repo,
        Read::Branch { branch: bytes("feature/slash") }.operation(),
        |a| a == &Answer::Commit(head),
    ));
    let mut names = ["actions", "conflict", "feature/slash", "landing", "main", "writer-head"].map(bytes).to_vec();
    if runner.is_none() {
        names.retain(|name| name.as_ref() != b"actions");
    }
    names.retain(|name| name.starts_with(b"m"));
    names.sort();
    facts.push(h.observe(
        "component-read-branches",
        repo,
        Read::Branches { prefix: Box::from(&b"m"[..]) }.operation(),
        |a| matches!(a, Answer::Branches(b) if { let mut b = b.to_vec(); b.sort(); b == names }),
    ));
    facts.push(h.observe("component-read-pull-files", repo, Read::PullFiles { number: f.feature, head, page: 1 }.operation(), |a| matches!(a, Answer::PullFiles { head: got, files, more: true } if *got == head && files.len() == 1 && files.iter().enumerate().all(|(i,f)| f.path == bytes(&format!("file-{i}.txt")) && f.before.is_none() && f.after == Some(bytes(&format!("file {i}\n")))))));
    facts.push(h.observe("component-read-compare", repo, Read::Compare { before: base, after: head }.operation(), |a| matches!(a, Answer::Compare { before, after, contains_before: true, files, commits } if *before == base && *after == head && commits.len() == 3 && files.len() == 3 && (0..3).all(|i| files.iter().any(|f| f.path == bytes(&format!("file-{i}.txt")) && f.before.is_none() && f.after == Some(bytes(&format!("file {i}\n"))))))));
    actions(runner, f, h, facts)?;
    facts.push(h.observe("component-read-file", repo, Read::File { head, path: bytes("file-0.txt"), max_bytes: 4 }.operation(), |a| matches!(a, Answer::File { head: got, path, bytes, truncated: true } if *got == head && path.as_ref() == b"file-0.txt" && bytes.as_ref() == b"file")));
    let protection = h.call(repo, Read::Protection { branch: bytes("main") }.operation());
    // protocol/forge.md 3.2 explicitly maps a writer's protection refusal to Forbidden.
    facts.push(super::fact("component-read-protection", protection == Err(api::Error::Forbidden), &protection));
    facts.push(h.observe(
        "component-read-settings",
        repo,
        Read::Settings.operation(),
        |a| matches!(a, Answer::Settings(s) if s.default_branch.as_ref() == b"main" && s.merge && s.squash && s.rebase),
    ));
    facts.push(h.observe("component-read-collaborators", repo, Read::Collaborators { page: 1 }.operation(), |a| matches!(a, Answer::Collaborators { collaborators, .. } if collaborators.iter().any(|c| c.user == f.writer.id && c.permission == api::Permission::Write))));
    facts.push(h.observe("component-read-permission", repo, Read::Permission { user: f.writer.id }.operation(), |a| {
        a == &Answer::Permission(api::Permission::Write)
    }));
    facts.push(h.observe(
        "component-read-locate",
        api::Repository { forge: 1, repository: 0 },
        Read::Locate { owner: bytes("fixture"), name: bytes("specimen") }.operation(),
        |a| a == &Answer::Located { repository: repo.repository },
    ));
    Ok(())
}
fn actions(runner: Option<&Path>, f: &Fixture, h: &mut Hosted, facts: &mut Vec<Fact>) -> Result<()> {
    if runner.is_none() {
        for name in ["component-read-checks", "component-read-job", JOB_END] {
            facts.push(Fact { name, outcome: crate::facts::Outcome::Unchecked("--runner not supplied".into()) });
        }
        return Ok(());
    }
    let head =
        commit(f.get(&format!("{R}/branches/actions"))?.root().get("commit").get("id").str().ok_or("actions head")?)?;
    let repo = repository(f)?;
    let checks = h.call(repo, Read::Checks { commit: head }.operation());
    let job = if let Ok(Answer::Checks(statuses)) = &checks { statuses.iter().find_map(|s| s.job) } else { None };
    facts.push(super::fact("component-read-checks", matches!(&checks, Ok(Answer::Checks(s)) if s.len() == 2 && s.iter().any(|s| s.check == api::Check::Passed) && s.iter().any(|s| s.check == api::Check::Failed && s.job.is_some_and(|j| j.head == head))), &checks));
    if let Some(attempt) = job {
        let document = f.get(&format!("{R}/actions/jobs/{}", attempt.job))?;
        let failed = document
            .root()
            .get("steps")
            .items()
            .into_iter()
            .find(|s| s.get("status").is("failure"))
            .ok_or("failed step")?;
        facts.push(h.observe("component-read-job", repo, Read::Job { attempt, max_bytes: 65536 }.operation(), |a| matches!(a, Answer::Job { attempt: got, step: Some(step), log, .. } if *got == attempt && Some(u64::from(step.number)) == failed.get("number").u64() && Some(step.name.as_ref()) == failed.get("name").str().map(str::as_bytes) && log.windows(b"KIT_CONFORMANCE_FAILED_JOB".len()).any(|s| s == b"KIT_CONFORMANCE_FAILED_JOB"))));
        let number = failed.get("number").u64().ok_or("Failed step number")?;
        let log = f.exchange(
            "GET",
            &format!("{R}/actions/jobs/{}/logs?attempt={}&step={number}", attempt.job, attempt.attempt),
            None,
            Some(&f.writer.token),
            None,
        )?;
        facts.push(h.observe(JOB_END, repo, Read::Job { attempt, max_bytes: 16 }.operation(), |a| matches!(a, Answer::Job { step: Some(step), log: kept, truncated: true, .. } if u64::from(step.number) == number && kept.len() == 16 && log.body.ends_with(kept))));
    } else {
        facts.push(super::fact("component-read-job", false, &"checks contained no failed job attempt"));
        facts.push(super::fact(JOB_END, false, &"Checks contained no failed job attempt"));
    }
    Ok(())
}
