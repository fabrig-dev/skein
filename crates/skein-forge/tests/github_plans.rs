#![cfg(test)]
use skein_forge::{
    api,
    github::{
        decode::Malformed,
        document::{self, Document, Row},
        failure::SECONDARY,
        header::{self, Head, RateFields},
        limits::Limits,
        pin,
        plans::{
            Body, Failure, Heard,
            plan::{Bounds, Exchange, Plan, Step},
            tables::{self, Tables},
        },
        request::Request,
    },
    measure,
};
use skein_json::{tokenizer, writer};
use skein_lib::{Duration, Wall};

const KEY: api::Repository = api::Repository { forge: 3, repository: 7 };
const SHA: &[u8] = b"0123456789abcdef0123456789abcdef01234567";
const DOCUMENT_BYTES: u32 = 4096;
const STRING_BYTES: u32 = 128;

fn limits() -> Limits {
    Limits {
        json: tokenizer::Limits { depth: 10, string: STRING_BYTES, number: 32, chunk: 7, length: DOCUMENT_BYTES },
        writer: writer::Limits { depth: 10, length: DOCUMENT_BYTES },
        string_bytes: STRING_BYTES,
        body_bytes: STRING_BYTES,
        inner: 4,
        target_bytes: 1024,
    }
}
fn bounds() -> Bounds {
    Bounds { inner: 8, page_size: 2, rows: 4, answer_bytes: 256, body_bytes: DOCUMENT_BYTES }
}
fn repository(id: u64) -> document::Repository {
    document::Repository {
        id,
        owner: Box::from(*b"owner"),
        owner_id: 9_007_199_254_740_993,
        organization: true,
        name: Box::from(*b"name"),
        default_branch: Box::from(*b"main"),
        html_url: Box::from(*b"https://example.invalid/owner/name"),
        merge: true,
        squash: false,
        rebase: true,
    }
}
fn keep(tables: &mut Tables) {
    tables.keep_repository(KEY, tables::repository(&repository(u64::from(KEY.repository))));
}
fn read_plan(read: api::Read) -> Plan {
    Plan::new(KEY, api::Op::Read(read))
}
fn branch() -> api::Read {
    api::Read::Branch { branch: Box::from(*b"feat/x y") }
}
fn branch_request() -> Request {
    Request::Branch { owner: Box::from(*b"owner"), name: Box::from(*b"name"), branch: Box::from(*b"feat/x y") }
}
fn branches_request(page: u32, per_page: u32) -> Request {
    Request::Branches { owner: Box::from(*b"owner"), name: Box::from(*b"name"), page, per_page }
}
fn head(plan: &mut Plan, next: bool, bounds: Bounds, tables: &mut Tables) {
    let mut head = Head::new();
    head.next = next;
    assert_eq!(plan.heard(Heard::Head { status: 200, head }, bounds, b"example-app", tables), Ok(None));
}
fn done(plan: &mut Plan, document: Document, bounds: Bounds, tables: &mut Tables) -> Result<Option<Step>, api::Error> {
    plan.heard(Heard::Done(Body::Document(document)), bounds, b"example-app", tables)
}
#[expect(clippy::unnecessary_wraps, reason = "expected verdicts retain heard's result shape beside call-order errors")]
fn ended(answer: Result<api::Answer, api::Error>) -> Result<Option<Step>, api::Error> {
    Ok(Some(Step::Done(answer)))
}
fn name(plan: &mut Plan, name: &[u8], bounds: Bounds, tables: &mut Tables) -> Result<Option<Step>, api::Error> {
    plan.heard(Heard::Row(Row::Branch(Box::from(name))), bounds, b"example-app", tables)
}
fn started(read: api::Read, bounds: Bounds, tables: &mut Tables) -> Plan {
    let mut plan = read_plan(read);
    match plan.begin(bounds, b"example-app", tables).expect("admitted read") {
        Step::Send(_) => {}
        Step::Done(answer) => panic!("expected request, got {answer:?}"),
        Step::Redirect { .. } => panic!("read begins with an API request"),
    }
    head(&mut plan, false, bounds, tables);
    plan
}

#[test]
fn github_plans_locate_keeps_the_row() {
    for (id, expected) in [
        (7, Ok(api::Answer::Located { repository: 7 })),
        (u64::from(u32::MAX), Ok(api::Answer::Located { repository: u32::MAX })),
        (u64::from(u32::MAX) + 1, Err(api::Error::TooLarge)),
        (0, Err(api::Error::InvalidAnswer)),
    ] {
        let key = api::Repository { repository: 0, ..KEY };
        let mut plan =
            Plan::new(key, api::Op::Read(api::Read::Locate { owner: Box::from(*b"o"), name: Box::from(*b"n") }));
        let mut tables = Tables::new(2, 2);
        assert_eq!(
            plan.begin(bounds(), b"example-app", &mut tables),
            Ok(Step::send(Request::Locate { owner: Box::from(*b"o"), name: Box::from(*b"n") }))
        );
        head(&mut plan, false, bounds(), &mut tables);
        let document = repository(id);
        assert_eq!(
            done(&mut plan, Document::Repository(document.clone()), bounds(), &mut tables),
            ended(expected.clone())
        );
        if let Ok(api::Answer::Located { repository }) = expected {
            assert_eq!(tables.repository(api::Repository { repository, ..KEY }), Some(&tables::repository(&document)));
            assert_eq!(tables.user(KEY.forge, document.owner_id), Some(document.owner.as_ref()));
        } else {
            assert_eq!(tables.repository(KEY), None);
            assert_eq!(tables.repository(key), None);
            assert_eq!(tables.user(KEY.forge, document.owner_id), None);
        }
    }
}

#[test]
fn github_plans_settings_refresh_the_row() {
    let mut tables = Tables::new(2, 2);
    keep(&mut tables);
    let mut plan = read_plan(api::Read::Settings);
    assert_eq!(plan.begin(bounds(), b"example-app", &mut tables), Ok(Step::send(Request::Repository { id: 7 })));
    head(&mut plan, false, bounds(), &mut tables);
    let mut document = repository(7);
    document.owner = Box::from(*b"new-owner");
    document.name = Box::from(*b"new-name");
    document.default_branch = Box::from(*b"trunk");
    document.merge = false;
    document.squash = true;
    assert_eq!(
        done(&mut plan, Document::Repository(document.clone()), bounds(), &mut tables),
        ended(Ok(api::Answer::Settings(api::Settings {
            default_branch: Box::from(*b"trunk"),
            merge: false,
            squash: true,
            rebase: true
        })))
    );
    assert_eq!(tables.repository(KEY), Some(&tables::repository(&document)));
    for (id, error) in
        [(8, api::Error::InvalidAnswer), (0, api::Error::InvalidAnswer), (u64::MAX, api::Error::TooLarge)]
    {
        let mut plan = started(api::Read::Settings, bounds(), &mut tables);
        assert_eq!(done(&mut plan, Document::Repository(repository(id)), bounds(), &mut tables), ended(Err(error)));
        assert_eq!(tables.repository(KEY), Some(&tables::repository(&document)));
        assert_eq!(tables.repository(api::Repository { repository: 8, ..KEY }), None);
    }
}

#[test]
fn github_plans_branch_resolves_then_reads() {
    let mut tables = Tables::new(2, 2);
    let mut plan = read_plan(branch());
    assert_eq!(plan.begin(bounds(), b"example-app", &mut tables), Ok(Step::send(Request::Repository { id: 7 })));
    head(&mut plan, false, bounds(), &mut tables);
    let document = repository(7);
    assert_eq!(
        done(&mut plan, Document::Repository(document.clone()), bounds(), &mut tables),
        Ok(Some(Step::send(branch_request())))
    );
    assert_eq!(tables.repository(KEY), Some(&tables::repository(&document)));
    head(&mut plan, false, bounds(), &mut tables);
    let commit = skein_forge::github::id::commit(SHA).expect("SHA-1 fixture");
    assert_eq!(
        done(
            &mut plan,
            Document::Branch(document::Branch { name: Box::from(*b"feat/x y"), commit }),
            bounds(),
            &mut tables
        ),
        ended(Ok(api::Answer::Commit(commit)))
    );
    let mut cached = read_plan(branch());
    assert_eq!(cached.begin(bounds(), b"example-app", &mut tables), Ok(Step::send(branch_request())));
}

#[test]
fn github_plans_branches_page_twice() {
    let mut tables = Tables::new(2, 2);
    let mut plan = read_plan(api::Read::Branches { prefix: Box::from(&b"m"[..]) });
    assert_eq!(plan.begin(bounds(), b"example-app", &mut tables), Ok(Step::send(Request::Repository { id: 7 })));
    head(&mut plan, false, bounds(), &mut tables);
    assert_eq!(
        done(&mut plan, Document::Repository(repository(7)), bounds(), &mut tables),
        Ok(Some(Step::send(branches_request(1, 2))))
    );
    head(&mut plan, true, bounds(), &mut tables);
    assert_eq!(name(&mut plan, b"main", bounds(), &mut tables), Ok(None));
    assert_eq!(name(&mut plan, b"m/feat/x y", bounds(), &mut tables), Ok(None));
    assert_eq!(done(&mut plan, Document::Rows, bounds(), &mut tables), Ok(Some(Step::send(branches_request(2, 2)))));
    head(&mut plan, false, bounds(), &mut tables);
    assert_eq!(name(&mut plan, b"m/trunk", bounds(), &mut tables), Ok(None));
    assert_eq!(name(&mut plan, b"m/deploy/x", bounds(), &mut tables), Ok(None));
    assert_eq!(
        done(&mut plan, Document::Rows, bounds(), &mut tables),
        ended(Ok(api::Answer::Branches(Box::from([
            Box::<[u8]>::from(*b"main"),
            Box::from(*b"m/feat/x y"),
            Box::from(*b"m/trunk"),
            Box::from(*b"m/deploy/x"),
        ]))))
    );
}

#[test]
fn github_plans_branches_past_the_bound() {
    let width = u32::try_from(measure::STRING_BYTES).expect("string width");
    for (bounds, first, second) in [
        (Bounds { rows: 1, ..bounds() }, b"a".as_slice(), b"aa".as_slice()),
        (Bounds { answer_bytes: width + 1, ..bounds() }, b"a".as_slice(), b"aa".as_slice()),
        (Bounds { answer_bytes: width * 2, ..bounds() }, b"12345678".as_slice(), b"123456789".as_slice()),
    ] {
        let mut tables = Tables::new(2, 2);
        keep(&mut tables);
        let mut plan = read_plan(api::Read::Branches { prefix: Box::from(&first[..1]) });
        plan.begin(bounds, b"example-app", &mut tables).expect("admitted");
        head(&mut plan, true, bounds, &mut tables);
        assert_eq!(name(&mut plan, first, bounds, &mut tables), Ok(None));
        // A fresh page separates the total-row/byte bound from the per-page bound.
        let per_page = bounds.page_size.min(bounds.rows).min(bounds.answer_bytes / width);
        assert_eq!(
            done(&mut plan, Document::Rows, bounds, &mut tables),
            Ok(Some(Step::send(branches_request(2, per_page))))
        );
        head(&mut plan, false, bounds, &mut tables);
        assert_eq!(name(&mut plan, second, bounds, &mut tables), ended(Err(api::Error::TooLarge)));
    }
}

fn dropped(status: u16, error: api::Error) {
    let mut tables = Tables::new(2, 2);
    keep(&mut tables);
    let mut plan = read_plan(branch());
    assert_eq!(plan.begin(bounds(), b"example-app", &mut tables), Ok(Step::send(branch_request())));
    assert_eq!(
        plan.heard(Heard::Head { status, head: Head::new() }, bounds(), b"example-app", &mut tables),
        ended(Err(error))
    );
    assert_eq!(tables.repository(KEY), None);
    let mut fresh = read_plan(branch());
    assert_eq!(fresh.begin(bounds(), b"example-app", &mut tables), Ok(Step::send(Request::Repository { id: 7 })));
}
#[test]
fn github_plans_branch_and_contents_missing_keep_the_row() {
    for status in [404, 410] {
        for read in [branch(), api::Read::File { head: [1; 32], path: Box::from(*b"file"), max_bytes: 16 }] {
            let mut tables = Tables::new(2, 2);
            keep(&mut tables);
            let mut plan = read_plan(read);
            plan.begin(bounds(), b"example-app", &mut tables).expect("leaf admitted");
            assert_eq!(
                plan.heard(Heard::Head { status, head: Head::new() }, bounds(), b"example-app", &mut tables),
                ended(Err(api::Error::Missing))
            );
            assert!(tables.repository(KEY).is_some());
            let mut fresh = read_plan(branch());
            assert_eq!(fresh.begin(bounds(), b"example-app", &mut tables), Ok(Step::send(branch_request())));
        }
    }
}
#[test]
fn github_plans_repository_missing_drops_the_row() {
    for status in [404, 410] {
        for read in [
            api::Read::Settings,
            api::Read::Branches { prefix: Box::from(&b"m"[..]) },
            api::Read::Item { number: 1, after: 0 },
        ] {
            let mut tables = Tables::new(2, 2);
            keep(&mut tables);
            let mut plan = read_plan(read);
            plan.begin(bounds(), b"example-app", &mut tables).expect("read admitted");
            assert_eq!(
                plan.heard(Heard::Head { status, head: Head::new() }, bounds(), b"example-app", &mut tables),
                ended(Err(api::Error::Missing))
            );
            assert_eq!(tables.repository(KEY), None);
            let mut fresh = read_plan(branch());
            assert_eq!(
                fresh.begin(bounds(), b"example-app", &mut tables),
                Ok(Step::send(Request::Repository { id: 7 }))
            );
        }
        for read in [branch(), api::Read::File { head: [1; 32], path: Box::from(*b"file"), max_bytes: 16 }] {
            let mut tables = Tables::new(2, 2);
            let mut plan = read_plan(read);
            assert_eq!(
                plan.begin(bounds(), b"example-app", &mut tables),
                Ok(Step::send(Request::Repository { id: 7 }))
            );
            keep(&mut tables);
            assert_eq!(
                plan.heard(Heard::Head { status, head: Head::new() }, bounds(), b"example-app", &mut tables),
                ended(Err(api::Error::Missing))
            );
            assert_eq!(tables.repository(KEY), None, "resolution is not a leaf");
        }
    }
}
#[test]
fn github_plans_moved_drops_the_row() {
    dropped(301, api::Error::Unavailable);
    dropped(307, api::Error::Unavailable);
}

#[test]
fn github_plans_rate_limit() {
    for status in [403, 429] {
        let mut primary = Head::new();
        primary.date = Some(Wall::from_nanos(100_000_000_000));
        primary.rate = Some(RateFields { limit: 5000, remaining: 0, reset: Wall::from_nanos(107_000_000_000) });
        let mut secondary = Head::new();
        header::field(&mut secondary, b"x-ratelimit-remaining", b"9", &limits());
        let mut retry = secondary.clone();
        header::field(&mut retry, b"Retry-After", b"3", &limits());
        for (head, body, after) in [
            (primary.clone(), Body::Empty, 7),
            (
                primary,
                Body::Document(Document::Error(document::Refusal {
                    message: Box::from(SECONDARY),
                    errors: Box::from([]),
                })),
                7,
            ),
            (
                secondary,
                Body::Document(Document::Error(document::Refusal {
                    message: Box::from(SECONDARY),
                    errors: Box::from([]),
                })),
                60,
            ),
            (retry, Body::Empty, 3),
        ] {
            let mut tables = Tables::new(2, 2);
            keep(&mut tables);
            let mut plan = read_plan(branch());
            plan.begin(bounds(), b"example-app", &mut tables).expect("admitted");
            assert_eq!(plan.heard(Heard::Head { status, head }, bounds(), b"example-app", &mut tables), Ok(None));
            assert_eq!(
                plan.heard(Heard::Done(body), bounds(), b"example-app", &mut tables),
                ended(Err(api::Error::RateLimited { after: Duration::from_secs(after) }))
            );
            assert_eq!(tables.repository(KEY), Some(&tables::repository(&repository(7))));
        }
        for failure in [Failure::Unseen, Failure::Seen] {
            let mut tables = Tables::new(2, 2);
            let mut plan = read_plan(api::Read::Settings);
            plan.begin(bounds(), b"example-app", &mut tables).expect("admitted");
            assert_eq!(
                plan.heard(Heard::Head { status, head: Head::new() }, bounds(), b"example-app", &mut tables),
                Ok(None)
            );
            assert_eq!(
                plan.heard(Heard::Failed(failure), bounds(), b"example-app", &mut tables),
                ended(Err(if status == 403 { api::Error::Forbidden } else { api::Error::InvalidAnswer }))
            );
        }
    }
}

#[test]
fn github_plans_transport_failures() {
    for (failure, error) in [(Failure::Unseen, api::Error::Unavailable), (Failure::Seen, api::Error::Timeout)] {
        for with_head in [false, true] {
            let mut tables = Tables::new(2, 2);
            let mut plan = read_plan(api::Read::Settings);
            plan.begin(bounds(), b"example-app", &mut tables).expect("admitted");
            if with_head {
                head(&mut plan, false, bounds(), &mut tables);
            }
            assert_eq!(plan.heard(Heard::Failed(failure), bounds(), b"example-app", &mut tables), ended(Err(error)));
        }
    }
}

#[test]
fn github_plans_malformed_body() {
    for (body, error) in [
        (Body::Malformed(Malformed::Shape { field: b"id" }), api::Error::InvalidAnswer),
        (Body::Malformed(Malformed::Bound { field: b"log" }), api::Error::TooLarge),
        (Body::Raw { bytes: Box::from(*b"bytes"), more: true }, api::Error::InvalidAnswer),
        (Body::Empty, api::Error::InvalidAnswer),
        (Body::Document(Document::Rows), api::Error::InvalidAnswer),
    ] {
        let mut tables = Tables::new(2, 2);
        let mut plan = started(api::Read::Settings, bounds(), &mut tables);
        assert_eq!(plan.heard(Heard::Done(body), bounds(), b"example-app", &mut tables), ended(Err(error)));
    }
}

#[test]
fn github_plans_job_and_protection_resolve_repository() {
    for read in [
        api::Read::Job { attempt: api::JobAttempt { head: [0; 32], run: 1, job: 1, attempt: 1 }, max_bytes: 1 },
        api::Read::Protection { branch: Box::from(*b"b") },
    ] {
        let mut tables = Tables::new(2, 2);
        assert_eq!(
            read_plan(read).begin(bounds(), b"example-app", &mut tables),
            Ok(Step::send(Request::Repository { id: 7 }))
        );
    }
}

#[test]
fn github_plans_page_size_and_empty_listing() {
    let width = u32::try_from(measure::STRING_BYTES).expect("string width");
    for (bounds, limit) in [
        (Bounds { page_size: 200, rows: 200, answer_bytes: 4000, ..bounds() }, pin::PAGE_CAP),
        (Bounds { page_size: 200, rows: 3, ..bounds() }, 3),
        (Bounds { page_size: 200, rows: 200, answer_bytes: width * 2, ..bounds() }, 2),
        (Bounds { page_size: 1, ..bounds() }, 1),
    ] {
        let mut tables = Tables::new(2, 2);
        keep(&mut tables);
        let mut plan = read_plan(api::Read::Branches { prefix: Box::from(&b"m"[..]) });
        assert_eq!(plan.begin(bounds, b"example-app", &mut tables), Ok(Step::send(branches_request(1, limit))));
        head(&mut plan, false, bounds, &mut tables);
        assert_eq!(
            done(&mut plan, Document::Rows, bounds, &mut tables),
            ended(Ok(api::Answer::Branches(Box::from([]))))
        );
    }
    for bounds in [
        Bounds { page_size: 0, ..bounds() },
        Bounds { rows: 0, ..bounds() },
        Bounds { answer_bytes: 0, ..bounds() },
        Bounds { answer_bytes: width - 1, ..bounds() },
    ] {
        let mut tables = Tables::new(0, 0);
        assert_eq!(
            read_plan(api::Read::Branches { prefix: Box::from(&b"m"[..]) }).begin(bounds, b"example-app", &mut tables),
            Err(api::Error::TooLarge)
        );
    }
}

#[test]
fn github_plans_page_row_bound() {
    let mut tables = Tables::new(2, 2);
    keep(&mut tables);
    let mut plan = started(api::Read::Branches { prefix: Box::from(&b"m"[..]) }, bounds(), &mut tables);
    for _ in 0..bounds().page_size {
        assert_eq!(name(&mut plan, b"a", bounds(), &mut tables), Ok(None));
    }
    assert_eq!(name(&mut plan, b"a", bounds(), &mut tables), ended(Err(api::Error::TooLarge)));
}

#[test]
fn github_plans_resolve_rejects_wrong_repository() {
    for read in [branch(), api::Read::Branches { prefix: Box::from(&b"m"[..]) }] {
        let mut tables = Tables::new(2, 2);
        let mut plan = started(read, bounds(), &mut tables);
        assert_eq!(
            done(&mut plan, Document::Repository(repository(8)), bounds(), &mut tables),
            ended(Err(api::Error::InvalidAnswer))
        );
        assert_eq!(tables.repository(KEY), None);
        assert_eq!(tables.repository(api::Repository { repository: 8, ..KEY }), None);
    }
}

#[test]
fn github_plans_settings_answer_bound() {
    for (answer_bytes, expected) in [
        (
            4,
            Ok(api::Answer::Settings(api::Settings {
                default_branch: Box::from(*b"main"),
                merge: true,
                squash: false,
                rebase: true,
            })),
        ),
        (3, Err(api::Error::TooLarge)),
    ] {
        let bounds = Bounds { answer_bytes, ..bounds() };
        let mut tables = Tables::new(2, 2);
        let mut plan = started(api::Read::Settings, bounds, &mut tables);
        assert_eq!(done(&mut plan, Document::Repository(repository(7)), bounds, &mut tables), ended(expected));
    }
}

#[test]
fn github_plans_status_and_call_order() {
    for (status, error) in [
        (201, api::Error::InvalidAnswer),
        (204, api::Error::InvalidAnswer),
        (401, api::Error::Forbidden),
        (500, api::Error::Timeout),
    ] {
        let mut tables = Tables::new(2, 2);
        let mut plan = read_plan(api::Read::Settings);
        assert_eq!(
            plan.heard(Heard::Failed(Failure::Unseen), bounds(), b"example-app", &mut tables),
            Err(api::Error::InvalidAnswer)
        );
        plan.begin(bounds(), b"example-app", &mut tables).expect("admitted");
        assert_eq!(plan.begin(bounds(), b"example-app", &mut tables), Err(api::Error::InvalidAnswer));
        assert_eq!(
            done(&mut plan, Document::Repository(repository(7)), bounds(), &mut tables),
            Err(api::Error::InvalidAnswer)
        );
        assert_eq!(
            plan.heard(Heard::Head { status, head: Head::new() }, bounds(), b"example-app", &mut tables),
            ended(Err(error))
        );
        assert_eq!(
            plan.heard(Heard::Failed(Failure::Seen), bounds(), b"example-app", &mut tables),
            Err(api::Error::InvalidAnswer)
        );
    }
    let mut tables = Tables::new(2, 2);
    let mut plan = started(api::Read::Settings, bounds(), &mut tables);
    assert_eq!(
        plan.heard(Heard::Head { status: 200, head: Head::new() }, bounds(), b"example-app", &mut tables),
        Err(api::Error::InvalidAnswer)
    );
    assert_eq!(
        done(&mut plan, Document::Repository(repository(7)), bounds(), &mut tables),
        ended(Ok(api::Answer::Settings(api::Settings {
            default_branch: Box::from(*b"main"),
            merge: true,
            squash: false,
            rebase: true
        })))
    );
    assert_eq!(
        done(&mut plan, Document::Repository(repository(7)), bounds(), &mut tables),
        Err(api::Error::InvalidAnswer)
    );
}

#[test]
fn github_plans_pending_is_caller_retained() {
    let mut tables = Tables::new(2, 2);
    let mut plan = read_plan(api::Read::Settings);
    let exchange = Exchange {
        request: Request::Repository { id: 7 },
        retention: None,
        comparison: skein_forge::github::plans::plan::Comparison::Full,
    };
    assert_eq!(plan.begin(bounds(), b"example-app", &mut tables), Ok(Step::Send(exchange.clone())));
    assert_eq!(plan.pending(), None);
    plan.set_pending(exchange.clone());
    assert_eq!(plan.pending(), Some(exchange));
    assert_eq!(plan.pending(), None);
}

#[test]
fn github_branch_prefix_keeps_two_matches_among_222_branches() {
    let mut tables = Tables::new(2, 2);
    keep(&mut tables);
    let bounds = Bounds { rows: 8, ..bounds() };
    let mut plan = read_plan(api::Read::Branches { prefix: Box::from(&b"temper/"[..]) });
    assert_eq!(plan.begin(bounds, b"example-app", &mut tables), Ok(Step::send(branches_request(1, 2))));
    for page in 1..=111 {
        head(&mut plan, page != 111, bounds, &mut tables);
        for index in [page * 2 - 2, page * 2 - 1] {
            let branch = match index {
                0 => Box::from(&b"temper/one"[..]),
                221 => Box::from(&b"temper/two"[..]),
                _ => Box::from(skein_lib::Decimal::of(u64::from(index)).as_bytes()),
            };
            assert_eq!(name(&mut plan, &branch, bounds, &mut tables), Ok(None));
        }
        let expected = if page == 111 {
            ended(Ok(api::Answer::Branches(Box::from([Box::from(&b"temper/one"[..]), Box::from(&b"temper/two"[..])]))))
        } else {
            Ok(Some(Step::send(branches_request(page + 1, 2))))
        };
        assert_eq!(done(&mut plan, Document::Rows, bounds, &mut tables), expected);
    }
}

#[test]
fn github_branch_prefix_ninth_match_exceeds_eight_rows() {
    let mut tables = Tables::new(2, 2);
    keep(&mut tables);
    let bounds = Bounds { rows: 8, ..bounds() };
    let mut plan = read_plan(api::Read::Branches { prefix: Box::from(&b"temper/"[..]) });
    plan.begin(bounds, b"example-app", &mut tables).expect("begin");
    for page in 1_u32..=5 {
        head(&mut plan, page != 5, bounds, &mut tables);
        for index in [page * 2 - 2, page * 2 - 1] {
            let result = name(&mut plan, &numbered_prefix(b"temper/", index), bounds, &mut tables);
            if index == 8 {
                assert_eq!(result, ended(Err(api::Error::TooLarge)));
                return;
            }
            assert_eq!(result, Ok(None));
        }
        done(&mut plan, Document::Rows, bounds, &mut tables).expect("page");
    }
    panic!("ninth row must refuse");
}

#[test]
fn github_empty_branch_prefix_is_named_empty_without_a_request() {
    let mut tables = Tables::new(2, 2);
    let mut plan = read_plan(api::Read::Branches { prefix: Box::new([]) });
    assert_eq!(plan.begin(bounds(), b"example-app", &mut tables), Err(api::Error::Empty));
    assert_eq!(plan.pending(), None);
}

fn numbered_prefix(prefix: &[u8], index: u32) -> Box<[u8]> {
    let digits = skein_lib::Decimal::of(u64::from(index));
    let mut out = skein_lib::Writer::new(prefix.len().checked_add(digits.as_bytes().len()).expect("fixture size"));
    out.put(prefix).unwrap();
    out.put(digits.as_bytes()).unwrap();
    out.finish()
}
