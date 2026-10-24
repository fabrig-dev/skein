//! Answering 404s retain cached paths, and listing headers govern their own page.
use super::{MISSING, replies, side_cases};
use crate::api::{self, Answer, Error, Op, Read};
use crate::forgejo::plans::plan::Step;
use crate::forgejo::tests::fixtures::{self, ATTEMPT, BASE, HEAD, JOB, PAGE_SIZE, PULL, RAW_BYTES, REPO, RUN, Run};
use alloc::boxed::Box;
use skein_lib::Time;

const FAILING: &[u8] = br#"{"state":"failure","total_count":1,"statuses":[{"creator":null,"updated_at":"1970-01-01T00:00:00Z","context":"build","status":"failure","description":"failed","target_url":"/fixture/specimen/actions/runs/5/jobs/1"}]}"#;
const RUNS: &[u8] = br#"{"total_count":1,"workflow_runs":[{"id":7,"index_in_repo":5,"commit_sha":"0100000000000000000000000000000000000000"}]}"#;

fn side_answers(file: api::File) -> [Answer; 2] {
    [
        Answer::PullFiles { head: HEAD, files: Box::new([file.clone()]), more: false },
        Answer::Compare {
            before: BASE,
            after: HEAD,
            contains_before: true,
            files: Box::new([file]),
            commits: Box::new([]),
        },
    ]
}
fn failed_check() -> api::Status {
    api::Status {
        at: Time::ZERO,
        context: Box::from(&b"build"[..]),
        check: api::Check::Failed,
        description: Box::from(&b"failed"[..]),
        url: Box::from(&b"http://127.0.0.1:3000/fixture/specimen/actions/runs/5/jobs/1"[..]),
        job: None,
    }
}

#[test]
fn answering_404_keeps_the_path() {
    for (before, after, statuses) in
        [(None, Some(Box::from(&b""[..])), [404, 200]), (Some(Box::from(&b""[..])), None, [200, 404])]
    {
        let file = api::File { path: Box::from(super::FILE_PATH), before, after };
        for ((read, answers, _sends), expected) in side_cases().into_iter().zip(side_answers(file)) {
            let mut run = Run::known(Op::Read(read));
            replies(&mut run, answers);
            run.answer(statuses[0], b"");
            assert_eq!(run.answer(statuses[1], b""), Step::Done(Ok(expected)));
            assert_eq!(run.tables().repository(REPO), Some(&fixtures::specimen()));
        }
    }
    for (name, answers) in [("run", &[(200, FAILING)][..]), ("job", &[(200, FAILING), (200, RUNS)][..])] {
        let mut run = Run::known(Op::Read(Read::Checks { commit: HEAD }));
        replies(&mut run, answers);
        assert_eq!(run.answer(404, MISSING), Step::Done(Ok(Answer::Checks(Box::new([failed_check()])))), "{name}");
        assert_eq!(run.tables().repository(REPO), Some(&fixtures::specimen()), "{name}");
    }
}

#[test]
fn terminal_404_drops_the_path() {
    let job = Read::Job { attempt: ATTEMPT, max_bytes: RAW_BYTES };
    for (name, read, answers, error) in [
        ("run", job.clone(), &[][..], Error::MissingJob),
        ("job", job.clone(), &[(200, RUN)][..], Error::MissingJob),
        ("log", job, &[(200, RUN), (200, JOB)][..], Error::MissingJob),
    ] {
        let mut run = Run::known(Op::Read(read));
        replies(&mut run, answers);
        assert_eq!(run.answer(404, MISSING), Step::Done(Err(error)), "{name}");
        assert_eq!(run.tables().repository(REPO), None, "{name}");
    }
}

#[test]
fn redirect_drops_the_path() {
    for (name, read, answers) in [
        ("file", Read::File { head: HEAD, path: Box::from(super::FILE_PATH), max_bytes: RAW_BYTES }, &[][..]),
        ("job", Read::Job { attempt: ATTEMPT, max_bytes: RAW_BYTES }, &[(200, RUN)][..]),
        ("checks", Read::Checks { commit: HEAD }, &[(200, FAILING)][..]),
        (
            "pull side",
            Read::PullFiles { number: 1, head: HEAD, page: 1 },
            &[(200, PULL), (200, super::CHANGED_FILES)][..],
        ),
        (
            "compare side",
            Read::Compare { before: BASE, after: HEAD },
            &[(200, super::CHANGED_COMPARE), (200, fixtures::EMPTY_COMPARE)][..],
        ),
    ] {
        let mut run = Run::known(Op::Read(read));
        replies(&mut run, answers);
        assert_eq!(run.answer(302, b""), Step::Done(Err(Error::Unavailable)), "{name}");
        assert_eq!(run.tables().repository(REPO), None, "{name}");
    }
}

#[test]
fn items_without_date_is_invalid_answer() {
    let mut run = Run::new(Op::Read(Read::Items { since: Time::ZERO, page: 1, kind: None }));
    run.begin();
    run.answer(200, fixtures::REPOSITORY);
    assert_eq!(run.answer_with(200, b"[]", fixtures::fields()), Step::Done(Err(Error::InvalidAnswer)));
    run.assert_sent(&[
        "GET /api/v1/repositories/1",
        "GET /api/v1/repos/fixture/specimen/issues?state=all&since=1970-01-01T00%3A00%3A00Z&sort=leastupdate&page=1&limit=2",
    ]);
}

// The full page contains PAGE_SIZE rows; the short page contains one fewer.
const FULL_FILES: &[u8] = br#"[{"filename":"x","status":"added"},{"filename":"y","status":"added"}]"#;
const SHORT_FILES: &[u8] = br#"[{"filename":"x","status":"added"}]"#;
const FULL_PATHS: [&[u8]; 2] = [b"x", b"y"];
fn added_file(path: &[u8]) -> api::File {
    api::File { path: Box::from(path), before: None, after: Some(Box::new([])) }
}
fn pull_files_page(body: &[u8], paths: &[&[u8]], has_more: Option<bool>) -> Step {
    let mut run = Run::known(Op::Read(Read::PullFiles { number: 1, head: HEAD, page: 1 }));
    run.begin();
    run.answer(200, PULL);
    let mut step = run.answer_with(200, body, crate::forgejo::plans::Fields { has_more, ..fixtures::fields() });
    for _path in paths {
        step = run.answer(200, b"");
    }
    step
}

#[test]
fn short_pull_files_without_has_more_is_last() {
    let short_rows = usize::try_from(PAGE_SIZE.saturating_sub(1)).unwrap();
    assert_eq!(
        pull_files_page(SHORT_FILES, &FULL_PATHS[..short_rows], None),
        Step::Done(Ok(Answer::PullFiles { head: HEAD, files: Box::new([added_file(b"x")]), more: false }))
    );
}

#[test]
fn full_pull_files_without_has_more_has_more() {
    assert_eq!(
        pull_files_page(FULL_FILES, &FULL_PATHS[..usize::try_from(PAGE_SIZE).unwrap()], None),
        Step::Done(Ok(Answer::PullFiles {
            head: HEAD,
            files: Box::new([added_file(b"x"), added_file(b"y")]),
            more: true
        }))
    );
}

#[test]
fn explicit_last_pull_files_page_has_no_more() {
    assert_eq!(
        pull_files_page(FULL_FILES, &FULL_PATHS[..usize::try_from(PAGE_SIZE).unwrap()], Some(false)),
        Step::Done(Ok(Answer::PullFiles {
            head: HEAD,
            files: Box::new([added_file(b"x"), added_file(b"y")]),
            more: false
        }))
    );
}

#[test]
fn branch_and_contents_404_keep_the_row() {
    for read in [
        Read::Branch { branch: Box::from(&b"topic"[..]) },
        Read::File { head: HEAD, path: Box::from(super::FILE_PATH), max_bytes: RAW_BYTES },
    ] {
        let mut run = Run::known(Op::Read(read));
        run.begin();
        assert_eq!(run.answer(404, MISSING), Step::Done(Err(Error::Missing)));
        assert_eq!(run.tables().repository(REPO), Some(&fixtures::specimen()));
    }
}

#[test]
fn repository_404_drops_the_row() {
    for read in [Read::Settings, Read::Item { number: 1, after: 0 }] {
        let mut run = Run::known(Op::Read(read));
        run.begin();
        assert_eq!(run.answer(404, MISSING), Step::Done(Err(Error::Missing)));
        assert_eq!(run.tables().repository(REPO), None);
    }
    for read in [
        Read::Branch { branch: Box::from(&b"topic"[..]) },
        Read::File { head: HEAD, path: Box::from(super::FILE_PATH), max_bytes: RAW_BYTES },
    ] {
        let mut run = Run::new(Op::Read(read));
        run.begin();
        run.tables().keep_repository(REPO, fixtures::specimen());
        assert_eq!(run.answer(404, MISSING), Step::Done(Err(Error::Missing)));
        assert_eq!(run.tables().repository(REPO), None, "resolution is not a leaf");
    }
}
