//! Document translations; the byte-level read-plan tests follow in 13.3b.
use alloc::boxed::Box;
use skein_forge::api;
use skein_forge::forgejo::document as d;
use skein_forge::forgejo::plans::reading;
use skein_lib::{Time, Wall};

#[test]
fn revisions_and_provenance_follow_the_body_and_its_times() {
    let rows = [
        (
            "equal",
            kept(),
            Some(Wall::from_nanos(1)),
            Some(Wall::from_nanos(1)),
            api::Provenance::Original,
            0xba78_16bf_8f01_cfea,
        ),
        (
            "later",
            kept(),
            Some(Wall::from_nanos(1)),
            Some(Wall::from_nanos(3)),
            api::Provenance::Revised,
            0xba78_16bf_8f01_cfea,
        ),
        ("missing", kept(), Some(Wall::from_nanos(1)), None, api::Provenance::Unknown, 0xba78_16bf_8f01_cfea),
        (
            "long",
            d::Body::Long(5),
            Some(Wall::from_nanos(1)),
            Some(Wall::from_nanos(3)),
            api::Provenance::Unknown,
            0x46c3_95ed_52b0_b56f,
        ),
    ];
    for (name, body, created, updated, provenance, revision) in rows {
        assert_eq!(reading::provenance(created, updated, &body), provenance, "{name}: provenance");
        assert_eq!(reading::revision(&body, updated), revision, "{name}: revision");
    }
    revisions_from_bytes();
}
fn kept() -> d::Body {
    d::Body::Kept(Box::from(&b"abc"[..]))
}

#[test]
fn an_actions_status_reads_with_no_creator_and_an_absolute_link() {
    let status = d::CommitStatus {
        creator: None,
        updated_at: Wall::from_nanos(2),
        context: Box::from(&b"build"[..]),
        status: d::StatusState::Failure,
        description: Box::from(&b"failed"[..]),
        target_url: Box::from(&b"/fixture/specimen/actions/runs/1/jobs/0"[..]),
    };
    assert_eq!(
        reading::status(status, b"http://127.0.0.1:3000/fixture/specimen"),
        Ok(api::Status {
            at: Time::from_nanos(2),
            context: Box::from(&b"build"[..]),
            check: api::Check::Failed,
            description: Box::from(&b"failed"[..]),
            url: Box::from(&b"http://127.0.0.1:3000/fixture/specimen/actions/runs/1/jobs/0"[..]),
            job: None
        }),
        "Actions path has its origin, no author or job"
    );
    actions_from_bytes();
}

#[test]
fn a_local_status_link_gets_its_repositorys_origin() {
    let rows: [(&str, &[u8], &[u8]); 3] = [
        ("local", b"/job", b"https://forge.test:443/job"),
        ("other origin", b"https://ci.test/job", b"https://ci.test/job"),
        ("empty", b"", b""),
    ];
    for (name, link, expected) in rows {
        assert_eq!(reading::absolute(link, b"https://forge.test:443/owner/repo"), Ok(Box::from(expected)), "{name}");
    }
}

#[test]
fn combined_statuses_map_their_state_and_zero_contexts_override_it() {
    let rows = [
        ("pending", d::StatusState::Pending, api::Ci::Pending),
        ("success", d::StatusState::Success, api::Ci::Passed),
        ("warning", d::StatusState::Warning, api::Ci::Passed),
        ("skipped", d::StatusState::Skipped, api::Ci::Passed),
        ("error", d::StatusState::Error, api::Ci::Failed),
        ("failure", d::StatusState::Failure, api::Ci::Failed),
    ];
    for (name, state, expected) in rows {
        assert_eq!(reading::ci(Some(state), 0), api::Ci::None, "{name}: no contexts");
        assert_eq!(reading::ci(Some(state), 1), expected, "{name}: contexts");
    }
    assert_eq!(reading::ci(None, 0), api::Ci::None, "empty state and no contexts");
}

#[test]
fn status_states_map_to_the_clients_check() {
    for (name, state, expected) in [
        ("pending", d::StatusState::Pending, api::Check::Pending),
        ("success", d::StatusState::Success, api::Check::Passed),
        ("warning", d::StatusState::Warning, api::Check::Passed),
        ("skipped", d::StatusState::Skipped, api::Check::Passed),
        ("error", d::StatusState::Error, api::Check::Failed),
        ("failure", d::StatusState::Failure, api::Check::Failed),
    ] {
        assert_eq!(reading::check(state), expected, "{name}");
    }
}

#[test]
fn pending_reviews_are_skipped_and_dismissed_reviews_are_comments() {
    for (name, state, dismissed, expected) in [
        ("approved", d::ReviewState::Approved, false, Some(api::Verdict::Approve)),
        ("changes", d::ReviewState::RequestChanges, false, Some(api::Verdict::RequestChanges)),
        ("comment", d::ReviewState::Comment, false, Some(api::Verdict::Comment)),
        ("pending", d::ReviewState::Pending, false, None),
        ("request", d::ReviewState::RequestReview, false, None),
        ("dismissed", d::ReviewState::Approved, true, Some(api::Verdict::Comment)),
    ] {
        assert_eq!(reading::verdict(state, dismissed), expected, "{name}");
    }
}

#[test]
fn an_owner_is_read_as_admin() {
    for (name, permission, expected) in [
        ("none", d::Permission::None, api::Permission::None),
        ("read", d::Permission::Read, api::Permission::Read),
        ("write", d::Permission::Write, api::Permission::Write),
        ("admin", d::Permission::Admin, api::Permission::Admin),
        ("owner", d::Permission::Owner, api::Permission::Admin),
    ] {
        assert_eq!(reading::permission(permission), expected, "{name}");
    }
}

#[test]
fn an_object_id_must_match_its_repositorys_format() {
    use skein_forge::forgejo::id::{ObjectFormat, ObjectId};
    for (name, from, repository, expected) in [
        ("sha1", ObjectFormat::Sha1, ObjectFormat::Sha1, Some([0; 32])),
        ("sha256", ObjectFormat::Sha256, ObjectFormat::Sha256, Some([0; 32])),
        ("another format", ObjectFormat::Sha1, ObjectFormat::Sha256, None),
    ] {
        assert_eq!(reading::commit(ObjectId { format: from, bytes: [0; 32] }, repository), expected, "{name}");
    }
}

#[test]
fn a_summary_keeps_its_marker_and_a_long_body_carries_no_key_or_bytes() {
    let rows = [
        (
            "kept",
            d::Body::Kept(Box::from(&b"<!-- kit:key 01 -->\ntext"[..])),
            Some(Box::from([1])),
            Box::from(&b"<!-- kit:key 01 -->\ntext"[..]),
        ),
        ("long", d::Body::Long(30), None, Box::from(&b""[..])),
    ];
    for (name, body, key, expected_body) in rows {
        let summary = reading::summary(
            fixtures::WORD,
            d::Issue {
                number: 7,
                pull: true,
                state: d::IssueState::Open,
                user: d::User { id: 2, login: Box::from(&b"writer"[..]), is_admin: false, full_name: Box::from([]) },
                labels: Box::from([]),
                updated_at: Wall::from_nanos(3),
                title: Box::from(&b"title"[..]),
                body,
            },
        );
        assert_eq!(
            summary,
            api::Summary {
                number: 7,
                kind: api::Kind::Pull,
                state: api::State::Open,
                author: 2,
                labels: Box::from([]),
                updated: Time::from_nanos(3),
                title: Box::from(&b"title"[..]),
                key,
                body: expected_body
            },
            "{name}"
        );
    }
}

use super::fixtures;
use api::{Answer, Op, Read};
use skein_lib::Duration;
const ISSUE: &[u8] = br#"{"number":1,"pull_request":null,"state":"open","user":{"id":2,"login":"writer"},"labels":[],"updated_at":"2020-01-01T00:00:01+05:30","title":"t","body":"b"}"#;
const EMPTY_STATUS: &[u8] = br#"{"state":"","total_count":0,"statuses":null}"#;
const SHA: [u8; 32] = [0; 32];
fn answer(h: &mut fixtures::Harness, body: &[u8]) {
    h.answer(fixtures::FORGE, &fixtures::ok_dated(body));
    h.run(Duration::ZERO);
}
fn last(h: &fixtures::Harness, expected: &[u8]) {
    assert_eq!(h.request_lines(fixtures::FORGE).last(), Some(&Box::from(expected)), "request target");
}
fn summary() -> api::Summary {
    api::Summary {
        labels: Box::from([]),
        number: 1,
        kind: api::Kind::Issue,
        state: api::State::Open,
        author: 2,
        key: None,
        title: Box::from(&b"t"[..]),
        body: Box::from(&b"b"[..]),
        updated: Time::from_nanos(1_577_817_001_000_000_000),
    }
}
fn listed(body: &[u8]) -> Box<[u8]> {
    let mut out = skein_lib::Writer::new(body.len().saturating_add(2));
    out.put(b"[").expect("list start");
    out.put(body).expect("row");
    out.put(b"]").expect("list end");
    out.finish()
}
#[test]
fn items_list_since_with_leastupdate_and_an_explicit_page() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::Items { since: Time::ZERO, page: 1, kind: None }));
    answer(&mut h, &listed(ISSUE));
    last(&h, b"GET /api/v1/repos/fixture/specimen/issues?state=all&since=1970-01-01T00%3A00%3A00Z&sort=leastupdate&page=1&limit=2 HTTP/1.1");
    assert_eq!(
        h.answered(1),
        Some(&crate::Answered {
            call: skein_lib::Token::new(1),
            cost: 1,
            free: 0,
            rate: None,
            result: Ok(Answer::Items {
                items: Box::new([summary()]),
                more: false,
                now: Time::from_nanos(1_577_836_800_000_000_000)
            })
        })
    );
}
#[test]
fn an_item_keeps_comments_above_after_until_the_answer_is_full() {
    let mut shared = fixtures::shared();
    shared.rows = 2;
    let mut h = fixtures::Harness::with(fixtures::limits(), shared, fixtures::settings());
    h.connected(fixtures::FORGE);
    answer(&mut h, br#"{"id":2,"login":"writer"}"#);
    answer(&mut h, br#"{"max_response_items":2,"default_paging_num":30}"#);
    h.known(fixtures::REPO, fixtures::specimen());
    h.call(1, fixtures::REPO, Op::Read(Read::Item { number: 1, after: 1 }));
    answer(&mut h, ISSUE);
    answer(&mut h, br#"[{"id":1,"user":{"id":2,"login":"writer"},"body":"skip"},{"id":2,"user":{"id":2,"login":"writer"},"body":"abc"},{"id":3,"user":{"id":2,"login":"writer"},"body":"abc"},{"id":4,"user":{"id":2,"login":"writer"},"body":"abc"}]"#);
    last(&h, b"GET /api/v1/repos/fixture/specimen/issues/1/comments HTTP/1.1");
    assert_eq!(h.answered(1).expect("item").cost, 2);
    assert_eq!(
        h.answered(1).expect("item").result,
        Ok(Answer::Item { item: summary(), comments: Box::new([comment(2), comment(3)]), more: true })
    );
}
fn comment(id: u64) -> api::Comment {
    api::Comment {
        id,
        provenance: api::Provenance::Unknown,
        author: 2,
        created: Time::ZERO,
        revision: 0xba78_16bf_8f01_cfea,
        key: None,
        body: Box::from(&b"abc"[..]),
    }
}
#[test]
fn a_call_without_a_kept_path_resolves_by_id_and_counts_it() {
    let mut h = fixtures::Harness::ready();
    h.call(1, fixtures::REPO, Op::Read(Read::Items { since: Time::ZERO, page: 1, kind: None }));
    answer(&mut h, br#"{"id":1,"owner":{"id":2,"login":"fixture"},"name":"specimen","object_format_name":"sha1","default_branch":"main","default_merge_style":"merge","allow_merge_commits":true,"allow_squash_merge":false,"allow_rebase":false,"html_url":"http://127.0.0.1:3000/fixture/specimen"}"#);
    answer(&mut h, b"[]");
    let lines = h.request_lines(fixtures::FORGE);
    assert_eq!(lines.get(2), Some(&Box::from(&b"GET /api/v1/repositories/1 HTTP/1.1"[..])), "resolution");
    assert_eq!(h.answered(1).expect("resolved answer").cost, 2);
    assert_eq!(
        h.answered(1).expect("resolved answer").result,
        Ok(Answer::Items { items: Box::from([]), more: false, now: Time::from_nanos(1_577_836_800_000_000_000) })
    );
}
#[test]
fn a_pending_review_is_skipped_but_fills_the_page() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::Reviews { number: 1, page: 1 }));
    answer(&mut h, br#"[{"id":1,"user":{"id":2,"login":"writer"},"state":"PENDING","commit_id":"0000000000000000000000000000000000000000","body":"","official":false,"dismissed":false},{"id":2,"user":{"id":2,"login":"writer"},"state":"REQUEST_REVIEW","commit_id":"0000000000000000000000000000000000000000","body":"","official":false,"dismissed":false}]"#);
    assert_eq!(h.answered(1).expect("reviews").result, Ok(Answer::Reviews { reviews: Box::from([]), more: true }));
    last(&h, b"GET /api/v1/repos/fixture/specimen/pulls/1/reviews?page=1&limit=2 HTTP/1.1");
}
#[test]
fn statuses_say_more_while_pages_fall_short_of_total_count() {
    let mut h = fixtures::known();
    for (name, page, total, expected_ci, expected) in [
        ("first", 1, br#"{"state":"pending","total_count":3,"statuses":[]}"#.as_slice(), api::Ci::Pending, true),
        ("later", 3, br#"{"state":"success","total_count":3,"statuses":[]}"#.as_slice(), api::Ci::Passed, false),
    ] {
        h.call(u64::from(page), fixtures::REPO, Op::Read(Read::Statuses { commit: SHA, page }));
        answer(&mut h, total);
        assert_eq!(
            h.answered(u64::from(page)).expect("statuses").result,
            Ok(Answer::Statuses { ci: expected_ci, statuses: Box::from([]), more: expected }),
            "{name}"
        );
    }
}
#[test]
fn a_malformed_read_answer_is_invalid_and_closes_its_connection() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::Branch { branch: Box::from(&b"main"[..]) }));
    answer(&mut h, b"{}");
    assert_eq!(h.answered(1).expect("malformed answer").result, Err(api::Error::InvalidAnswer));
    assert_eq!(h.answered(1).expect("malformed answer").cost, 1);
    assert!(h.peers.is_empty(), "untrusted stream closed");
}
fn pull(number: u64, state: &[u8], head: &[u8]) -> Box<[u8]> {
    joined(&[b"{\"number\":", skein_lib::Decimal::of(number).as_bytes(), b",\"state\":\"", state, b"\",\"head\":{\"repo_id\":1,\"ref\":\"", head, b"\",\"sha\":\"0000000000000000000000000000000000000000\"},\"base\":{\"repo_id\":1,\"ref\":\"main\",\"sha\":\"\"},\"merged\":false,\"merge_commit_sha\":null,\"merge_base\":null,\"mergeable\":true,\"requested_reviewers\":[]}"])
}
fn joined(pieces: &[&[u8]]) -> Box<[u8]> {
    let mut length = 0_usize;
    for piece in pieces {
        length = length.saturating_add(piece.len());
    }
    let mut out = skein_lib::Writer::new(length);
    for piece in pieces {
        out.put(piece).expect("pieces measured");
    }
    out.finish()
}
fn expected_pull(number: u64, state: api::State) -> api::Pull {
    api::Pull {
        queued: None,
        dequeued: None,
        reviewers: Box::from([]),
        number,
        state,
        head: Box::from(&b"work"[..]),
        base: Box::from(&b"main"[..]),
        commit: SHA,
        base_commit: None,
        merged: None,
        mergeable: true,
        ci: api::Ci::None,
    }
}
#[test]
fn pull_for_lists_open_pulls_when_the_lookup_names_a_closed_one() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::PullFor { head: Box::from(&b"work"[..]), base: Box::from(&b"main"[..]) }));
    answer(&mut h, &pull(12, b"closed", b"work"));
    last(&h, b"GET /api/v1/repos/fixture/specimen/pulls?state=open&sort=oldest&page=1&limit=2 HTTP/1.1");
    answer(&mut h, &joined(&[b"[", &pull(9, b"open", b"other"), b",", &pull(13, b"open", b"work"), b"]"]));
    last(&h, b"GET /api/v1/repos/fixture/specimen/commits/0000000000000000000000000000000000000000/status?page=1&limit=1 HTTP/1.1");
    answer(&mut h, EMPTY_STATUS);
    assert_eq!(h.answered(1).expect("open pull").result, Ok(Answer::Pull(expected_pull(13, api::State::Open))));
    assert_eq!(h.answered(1).expect("open pull").cost, 3);
}
#[test]
fn pull_for_answers_the_lookups_closed_pull_when_none_is_open() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::PullFor { head: Box::from(&b"work"[..]), base: Box::from(&b"main"[..]) }));
    answer(&mut h, &pull(12, b"closed", b"work"));
    answer(&mut h, &listed(&pull(9, b"open", b"other")));
    answer(&mut h, EMPTY_STATUS);
    assert_eq!(h.answered(1).expect("closed pull").result, Ok(Answer::Pull(expected_pull(12, api::State::Closed))));
    assert_eq!(h.answered(1).expect("closed pull").cost, 3);
}

fn revisions_from_bytes() {
    let mut h = fixtures::known();
    let long = [b'x'; 1046];
    let rows = [
        (
            "equal",
            1,
            b"abc".as_slice(),
            b"\"1970-01-01T00:00:01Z\"".as_slice(),
            api::Provenance::Original,
            0xba78_16bf_8f01_cfea,
            b"abc".as_slice(),
        ),
        (
            "later",
            2,
            b"abc".as_slice(),
            b"\"1970-01-01T00:00:03Z\"".as_slice(),
            api::Provenance::Revised,
            0xba78_16bf_8f01_cfea,
            b"abc".as_slice(),
        ),
        (
            "missing",
            3,
            b"abc".as_slice(),
            b"null".as_slice(),
            api::Provenance::Unknown,
            0xba78_16bf_8f01_cfea,
            b"abc".as_slice(),
        ),
        (
            "long",
            4,
            long.as_slice(),
            b"\"1970-01-01T00:00:03Z\"".as_slice(),
            api::Provenance::Unknown,
            0x6dec_dbb8_004b_59a2,
            b"".as_slice(),
        ),
    ];
    for (name, call, body, updated, provenance, revision, expected_body) in rows {
        h.call(call, fixtures::REPO, Op::Read(Read::Item { number: 1, after: 0 }));
        answer(&mut h, ISSUE);
        let comments = joined(&[
            br#"[{"id":1,"user":{"id":2,"login":"writer"},"created_at":"1970-01-01T00:00:01Z","updated_at":"#,
            updated,
            br#", "body":""#,
            body,
            br#""}]"#,
        ]);
        answer(&mut h, &comments);
        assert_eq!(
            h.answered(call).expect("comment revision").result,
            Ok(Answer::Item {
                item: summary(),
                comments: Box::new([api::Comment {
                    id: 1,
                    author: 2,
                    created: Time::from_nanos(1_000_000_000),
                    revision,
                    provenance,
                    key: None,
                    body: Box::from(expected_body)
                }]),
                more: false
            }),
            "{name}"
        );
    }
}
fn measured_answers() -> [(&'static str, Answer); 10] {
    [
        ("items", Answer::Items { items: Box::new([summary()]), more: false, now: Time::ZERO }),
        ("item", Answer::Item { item: summary(), comments: Box::new([comment(2), comment(3)]), more: true }),
        ("pull", Answer::Pull(expected_pull(13, api::State::Open))),
        ("reviews", Answer::Reviews { reviews: Box::from([]), more: true }),
        ("statuses", Answer::Statuses { ci: api::Ci::None, statuses: Box::from([]), more: false }),
        (
            "Actions status",
            Answer::Statuses { ci: api::Ci::Failed, statuses: Box::new([actions_status()]), more: false },
        ),
        ("branches", Answer::Branches(Box::new([Box::from(&b"main"[..])]))),
        (
            "protection",
            Answer::Protection(Some(api::Protection {
                queue: false,
                signatures: false,
                linear: false,
                code_owners: false,
                bypass: api::Bypass::Never,
                branch: Box::from(&b"main"[..]),
                contexts: Box::new([Box::from(&b"build"[..])]),
                approvals: 2,
                dismiss_stale: true,
            })),
        ),
        (
            "settings",
            Answer::Settings(api::Settings {
                default_branch: Box::from(&b"main"[..]),
                merge: true,
                squash: false,
                rebase: false,
            }),
        ),
        (
            "collaborators",
            Answer::Collaborators {
                collaborators: Box::new([api::Collaborator { user: 2, permission: api::Permission::Admin }]),
                more: false,
            },
        ),
    ]
}
#[test]
fn the_running_answer_size_agrees_with_the_clients_measure() {
    for (name, answer) in measured_answers() {
        assert_eq!(
            skein_forge::forgejo::plans::reads::size::answer(&answer, 8).ok(),
            skein_forge::measure::answer(&answer),
            "{name}"
        );
    }
}
fn actions_from_bytes() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::Statuses { commit: SHA, page: 1 }));
    answer(&mut h, br#"{"state":"failure","total_count":1,"statuses":[{"creator":null,"updated_at":"1970-01-01T00:00:02Z","context":"build","status":"failure","description":"failed","target_url":"/fixture/specimen/actions/runs/1/jobs/0"}]}"#);
    assert_eq!(
        h.answered(1).expect("Actions status").result,
        Ok(Answer::Statuses { ci: api::Ci::Failed, more: false, statuses: Box::new([actions_status()]) })
    );
    assert_eq!(h.answered(1).expect("Actions status").cost, 1);
}
fn actions_status() -> api::Status {
    api::Status {
        at: Time::from_nanos(2_000_000_000),
        context: Box::from(&b"build"[..]),
        check: api::Check::Failed,
        description: Box::from(&b"failed"[..]),
        url: Box::from(&b"http://127.0.0.1:3000/fixture/specimen/actions/runs/1/jobs/0"[..]),
        job: None,
    }
}
const REPOSITORY: &[u8] = br#"{"id":1,"owner":{"id":2,"login":"fixture"},"name":"specimen","object_format_name":"sha1","default_branch":"main","default_merge_style":"merge","allow_merge_commits":true,"allow_squash_merge":false,"allow_rebase":false,"html_url":"http://127.0.0.1:3000/fixture/specimen"}"#;
const PERMISSION: &[u8] = br#"{"permission":"owner","user":{"id":2,"login":"writer"}}"#;
fn branch(name: &[u8], rule: &[u8]) -> Box<[u8]> {
    joined(&[
        br#"{"name":""#,
        name,
        br#"","commit":{"id":"0000000000000000000000000000000000000000"},"effective_branch_protection_name":""#,
        rule,
        br#""}"#,
    ])
}
#[test]
fn branches_round_trip_the_prefix_in_one_refs_request() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::Branches { prefix: Box::from(&b"temper/"[..]) }));
    h.run(Duration::ZERO);
    last(&h, b"GET /api/v1/repos/fixture/specimen/git/refs/heads/temper%2F HTTP/1.1");
    answer(&mut h, br#"[{"ref":"refs/heads/temper/one"},{"ref":"refs/heads/temper/two"}]"#);
    assert_eq!(
        h.answered(1).expect("branch names").result,
        Ok(Answer::Branches(Box::new([Box::from(&b"temper/one"[..]), Box::from(&b"temper/two"[..])])))
    );
    assert_eq!(h.answered(1).expect("branch names").cost, 1);
}

#[test]
fn protection_checks_admin_access_even_when_no_rule_applies() {
    let mut h = fixtures::known();
    for (name, call, rule, response, expected) in [
        ("no rule", 1, b"".as_slice(), b"[]".as_slice(), Ok(Answer::Protection(None))),
        ("effective rule", 2, b"main".as_slice(), br#"{"rule_name":"main","enable_status_check":true,"status_check_contexts":["build"],"required_approvals":2,"dismiss_stale_approvals":true,"require_signed_commits":false}"#.as_slice(), Ok(Answer::Protection(Some(api::Protection { queue: false, signatures: false, linear: false, code_owners: false, bypass: api::Bypass::Never, branch: Box::from(&b"main"[..]), contexts: Box::new([Box::from(&b"build"[..])]), approvals: 2, dismiss_stale: true })))),
    ] {
        h.call(call, fixtures::REPO, Op::Read(Read::Protection { branch: Box::from(&b"main"[..]) })); answer(&mut h, &branch(b"main", rule)); answer(&mut h, response);
        assert_eq!(h.answered(call).expect("protection").result, expected, "{name}");
        assert_eq!(h.answered(call).expect("protection").cost, 2, "{name}: cost");
    }
    h.call(3, fixtures::REPO, Op::Read(Read::Protection { branch: Box::from(&b"main"[..]) }));
    answer(&mut h, &branch(b"main", b""));
    h.answer(fixtures::FORGE, &fixtures::status(403));
    h.run(Duration::ZERO);
    assert_eq!(h.answered(3).expect("nonadmin absent rule").result, Err(api::Error::Forbidden));
}
#[test]
fn a_permission_resolves_its_user_and_reuses_the_kept_login() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::Permission { user: 2 }));
    h.run(Duration::ZERO);
    last(&h, b"GET /api/v1/users/search?uid=2&limit=1 HTTP/1.1");
    answer(&mut h, br#"{"ok":true,"data":[{"id":2,"login":"writer"}]}"#);
    last(&h, b"GET /api/v1/repos/fixture/specimen/collaborators/writer/permission HTTP/1.1");
    answer(&mut h, PERMISSION);
    assert_eq!(h.answered(1).expect("permission").result, Ok(Answer::Permission(api::Permission::Admin)));
    assert_eq!(h.answered(1).expect("permission").cost, 2);
    h.call(2, fixtures::REPO, Op::Read(Read::Permission { user: 2 }));
    answer(&mut h, PERMISSION);
    assert_eq!(h.answered(2).expect("cached login").cost, 1);
}
#[test]
fn collaborators_read_each_listed_users_permission() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::Collaborators { page: 1 }));
    answer(&mut h, br#"[{"id":2,"login":"writer"}]"#);
    answer(&mut h, PERMISSION);
    assert_eq!(
        h.answered(1).expect("collaborators").result,
        Ok(Answer::Collaborators {
            collaborators: Box::new([api::Collaborator { user: 2, permission: api::Permission::Admin }]),
            more: false
        })
    );
    assert_eq!(h.answered(1).expect("collaborators").cost, 2);
}
#[test]
fn locate_keeps_the_repository() {
    let mut h = fixtures::Harness::ready();
    h.call(
        1,
        api::Repository { forge: fixtures::FORGE, repository: 0 },
        Op::Read(Read::Locate { owner: Box::from(&b"fixture"[..]), name: Box::from(&b"specimen"[..]) }),
    );
    answer(&mut h, REPOSITORY);
    assert_eq!(h.answered(1).expect("located").result, Ok(Answer::Located { repository: 1 }));
    assert_eq!(h.answered(1).expect("located").cost, 1);
    assert!(h.component.forgejo_tables().repository(fixtures::REPO).is_some(), "located row kept");
}
#[test]
fn settings_refresh_the_kept_repository() {
    let mut h = fixtures::known();
    h.call(2, fixtures::REPO, Op::Read(Read::Settings));
    answer(&mut h, REPOSITORY);
    assert_eq!(
        h.answered(2).expect("settings").result,
        Ok(Answer::Settings(api::Settings {
            default_branch: Box::from(&b"main"[..]),
            merge: true,
            squash: false,
            rebase: false
        }))
    );
    assert_eq!(h.answered(2).expect("settings").cost, 1);
    assert_eq!(
        h.component.forgejo_tables().repository(fixtures::REPO).expect("refreshed row").merge_style,
        d::MergeStyle::Merge
    );
}
#[test]
fn remarks_select_the_requested_slice_and_cut_after_it() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::Remarks { number: 1, review: 2, page: 2 }));
    let row = br#"{"id":3,"user":{"id":2,"login":"writer"},"path":"file","position":4,"body":"abc"}"#;
    answer(&mut h, &joined(&[b"[", row, b",", row, b",", row, b",", row, b",", row, b"]"]));
    let remark = api::Remark { id: 3, author: 2, path: Box::from(&b"file"[..]), line: 4, body: Box::from(&b"abc"[..]) };
    assert_eq!(
        h.answered(1).expect("remarks").result,
        Ok(Answer::Remarks { remarks: Box::new([remark.clone(), remark]), more: true })
    );
}
#[test]
fn a_calls_deadline_distinguishes_connecting_from_a_written_request() {
    for (name, connect, error, cost) in
        [("connecting", false, api::Error::Unavailable, 0), ("written", true, api::Error::Timeout, 1)]
    {
        let mut h = fixtures::known();
        h.run(Duration::from_secs(5));
        h.call(1, fixtures::REPO, Op::Read(Read::Settings));
        h.run(Duration::ZERO);
        if connect {
            h.connected(fixtures::FORGE);
        }
        h.run(Duration::from_secs(10));
        assert_eq!(h.answered(1).expect("deadline").result, Err(error), "{name}");
        assert_eq!(h.answered(1).expect("deadline").cost, cost, "{name}: cost");
        assert!(h.peers.is_empty(), "{name}: connection aborted");
    }
}
#[test]
fn closing_an_active_call_discards_it_and_ends_its_socket() {
    let mut h = fixtures::known();
    h.call(1, fixtures::REPO, Op::Read(Read::Branch { branch: Box::from(&b"main"[..]) }));
    h.run(Duration::ZERO);
    h.send(crate::Request::Close);
    h.run(Duration::ZERO);
    assert!(h.closed, "close terminal");
    assert!(h.peers.is_empty(), "no socket owned");
    assert!(h.answered(1).is_none(), "no call answer after Close");
}

#[test]
fn json_body_demands_are_batched_across_bounded_pumps() {
    use crate::connection::Phase;
    use skein_forge::forgejo::request;
    // Many structural bytes exceed one pump's 64 movements while the
    // whole body fits the local intake. Whitespace after the object must
    // be checked before the HTTP terminal is consumed.
    let body = joined(&[br#"{"id":2,"login":"writer","ignored":["#, &fixtures::filled(180, b' '), b"]}"]);
    let response = fixtures::ok(&body);
    let mut h = fixtures::ConnectionHarness::new();
    h.connection
        .start(&h.env, &h.forge, fixtures::USER_AGENT, fixtures::forgejo_exchange(request::Request::User, None))
        .expect("user request");
    let heard = h.buffered_answer(&response, response.len(), false);
    user_decoded(&heard);
    assert_eq!(h.connection.phase, Phase::Idle);
    assert!(!h.connection.ready(), "no ready work after the terminal");
    assert!(h.local_pumps > 1, "buffered work survives a pump boundary");
    assert_eq!(h.body_demands, 1, "one buffered body batch");
}

fn user_decoded(heard: &skein_lib::List<crate::connection::Heard>) {
    use crate::connection::Heard;
    use skein_forge::forgejo::plans;
    assert_eq!(heard.len(), 2, "one head and one terminal: {heard:?}");
    assert_eq!(
        heard.get(1),
        Some(&Heard::Forgejo(plans::Heard::Done(plans::Body::Document(d::Document::User(d::User {
            id: 2,
            login: Box::from(&b"writer"[..]),
            is_admin: false,
            full_name: Box::from([]),
        }))))),
        "complete decoded body"
    );
}

fn user_request(h: &mut fixtures::ConnectionHarness) {
    h.connection
        .start(
            &h.env,
            &h.forge,
            fixtures::USER_AGENT,
            fixtures::forgejo_exchange(skein_forge::forgejo::request::Request::User, None),
        )
        .expect("user request");
}

#[test]
fn json_body_fragments_and_final_bytes_precede_end() {
    let response = fixtures::ok(br#"{"id":2,"login":"wri\u0074er","ignored":false}"#);
    for chunk in [1, 4, 1024] {
        for fragment in [1, 7, response.len()] {
            for eof in [false, true] {
                let mut h = fixtures::ConnectionHarness::with_chunk(chunk);
                user_request(&mut h);
                let heard = h.buffered_answer(&response, fragment, eof);
                user_decoded(&heard);
                assert!(!h.connection.ready(), "chunk{chunk}/fragment{fragment}/eof{eof}: no residual work");
                assert!(h.body_demands > 0, "body read");
            }
        }
    }
}

#[test]
fn json_body_batches_cross_intake_boundaries_and_reuse_cleanly() {
    let body = joined(&[br#"{"id":2,"login":"writer","ignored":["#, &fixtures::filled(2500, b' '), b"]}"]);
    let response = fixtures::ok(&body);
    let mut h = fixtures::ConnectionHarness::new();
    user_request(&mut h);
    user_decoded(&h.buffered_answer(&response, response.len(), false));
    let capacity = usize::try_from(h.env.limits.chunk).expect("chunk fits");
    assert_eq!(usize::try_from(h.body_demands).expect("count fits"), body.len().div_ceil(capacity));
    assert!(h.local_pumps > 2, "local work survives repeated 64-movement caps");
    assert_eq!(h.connection.phase, crate::connection::Phase::Idle);
    // A second exchange has no held bytes, end or demand from the first.
    user_request(&mut h);
    user_decoded(&h.buffered_answer(&fixtures::user_ok(), 7, false));
    assert_eq!(h.connection.phase, crate::connection::Phase::Idle);
}

#[test]
fn closing_with_a_partial_json_batch_drops_local_work() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{\"id\":2,\"login\":\"wri";
    let mut h = fixtures::ConnectionHarness::new();
    user_request(&mut h);
    let heard = h.buffered_answer(response, response.len(), false);
    assert_eq!(heard.len(), 1, "only the head before the incomplete string");
    assert!(!h.connection.ready(), "waits for bytes, not a local spin");
    h.connection.close(true, &mut h.io);
    h.connection.stream(&h.env, skein_lib::stream::Up::End);
    h.pump();
    assert!(!h.connection.ready(), "close leaves no local work");
    assert!(h.connection.heard().is_none(), "no answer after close");
}

#[test]
#[expect(
    clippy::print_stdout,
    clippy::disallowed_macros,
    reason = "report the local intake peak and teardown for the memory ledger"
)]
fn json_body_intakes_fit_the_worst_case_and_release_on_teardown() {
    let bound = crate::limits::worst_case(
        &fixtures::limits(),
        &fixtures::shared(),
        crate::limits::Forges { forgejo: 1, github: 0 },
        0,
    )
    .expect("complete bound");
    let span = skein_heap::Span::start();
    {
        let mut connections = skein_lib::List::with_capacity(fixtures::limits().connections);
        for _ in 0..fixtures::limits().connections {
            let mut h = fixtures::ConnectionHarness::new();
            user_request(&mut h);
            let body = joined(&[br#"{"id":2,"login":"writer","ignored":["#, &fixtures::filled(2500, b' '), b"]}"]);
            let response = fixtures::ok(&body);
            user_decoded(&h.buffered_answer(&response, response.len(), false));
            connections.push(h).expect("all configured connections");
        }
        for at in 0..connections.len() {
            let h = connections.get_mut(at).expect("configured connection");
            h.connection.close(false, &mut h.io);
            assert!(!h.connection.ready(), "closed connections do not spin");
        }
    }
    let grown = span.end();
    assert_eq!(grown.net, 0, "all fixture and local intake allocations released");
    assert!(u64::try_from(grown.peak).expect("positive peak") <= bound, "{} <= {bound}", grown.peak);
    println!("JSON intake fixture peak={} bound={bound} teardown_net={}", grown.peak, grown.net);
}
