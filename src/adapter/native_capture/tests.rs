use super::*;
#[test]
fn standard_hmac_sha256_rfc4231_vector() {
    let mut mac = Signature::new_from_slice(&[0x0b; 20]).unwrap();
    mac.update(b"Hi There");
    assert_eq!(
        hex(&mac.finalize().into_bytes()),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
}

fn fixture() -> (PathBuf, Workspace, HostContext) {
    let root = std::env::temp_dir().join(crate::learner::store::new_id("native-ticket").unwrap());
    fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
    fs::write(
        root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
        "schema_version = 1\nworkspace_id = \"native-ticket\"\n",
    )
    .unwrap();
    let workspace = Workspace::discover(&root).unwrap().unwrap();
    let host = HostContext {
        host_kind: "claude".into(),
        host_thread_id: "magnesium-h1:claude:conversation".into(),
    };
    (root, workspace, host)
}

#[test]
fn short_ticket_verifies_only_the_exact_current_turn_text_and_conversation() {
    for (text, altered) in [
        (
            "Teach me confidence intervals.",
            &["Teach me confidence intervals!"][..],
        ),
        (
            "I cannot identify b; \u{2212}b is not +b. π ≠ x.",
            &[
                "I can identify b; \u{2212}b is not +b. π ≠ x.",
                "I cannot identify b; -b is not +b. π ≠ x.",
            ][..],
        ),
    ] {
        let (root, workspace, host) = fixture();
        let ticket = issue(&workspace, &host, "turn-1", text, 1_000).unwrap();
        assert_eq!(ticket.len(), 32);
        assert!(ticket.bytes().all(|b| b.is_ascii_hexdigit()));
        // A retried prompt hook for the same turn reissues the same ticket.
        assert_eq!(
            issue(&workspace, &host, "turn-1", text, 2_000).unwrap(),
            ticket
        );

        let event = verify(&workspace, &host, &ticket, text, 1_500).unwrap();
        assert_eq!(event.text, text);
        assert_eq!(event.external_event_key, event_key("turn-1"));
        assert_eq!(event.occurred_at_ms, None);

        let other = HostContext {
            host_thread_id: "magnesium-h1:claude:other".into(),
            ..host.clone()
        };
        let flip = |index: usize| {
            let mut bytes = ticket.clone().into_bytes();
            bytes[index] = if bytes[index] == b'0' { b'1' } else { b'0' };
            String::from_utf8(bytes).unwrap()
        };
        let (flipped_last, flipped_middle) = (flip(31), flip(20));
        let mut rejected = vec![
            (&host, flipped_last.as_str(), text, 1_500),
            (&host, flipped_middle.as_str(), text, 1_500),
            (&host, &ticket[..31], text, 1_500),
            (&other, ticket.as_str(), text, 1_500),
            (&host, ticket.as_str(), text, 1_000 + TTL_MS + 1),
            (&host, ticket.as_str(), text, 999),
        ];
        rejected.extend(
            altered
                .iter()
                .map(|altered| (&host, ticket.as_str(), *altered, 1_500)),
        );
        for (host, ticket, text, now) in rejected {
            assert_eq!(
                verify(&workspace, host, ticket, text, now).err(),
                Some(failure())
            );
        }

        // A newer prompt replaces the marker, so the earlier ticket expires with it.
        issue(&workspace, &host, "turn-2", "Next question", 3_000).unwrap();
        assert_eq!(
            verify(&workspace, &host, &ticket, text, 3_500).err(),
            Some(failure())
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn the_window_opener_is_certified_as_the_goal_whichever_way_the_tutor_echoes_it() {
    let (root, workspace, host) = fixture();
    // The study window opens a lesson with this line. The hook sees all of
    // it; the tutor sees the skill the command expands to, and may echo the
    // whole line or only the goal.
    let opener = "/magnesium:learn adding fractions with different denominators";
    let goal = "adding fractions with different denominators";
    let ticket = issue(&workspace, &host, "turn-1", opener, 1_000).unwrap();
    for echo in [
        opener,
        goal,
        "/magnesium:learn  adding fractions with different denominators\n",
    ] {
        let event = verify(&workspace, &host, &ticket, echo, 1_500).unwrap();
        assert_eq!(event.text, goal, "echo {echo:?}");
    }
    // The ticket still shows the answer is what the learner typed.
    assert_eq!(
        verify(&workspace, &host, &ticket, "adding fractions", 1_500).err(),
        Some(failure())
    );
    for (raw, stored) in [
        ("/magnesium:papers midterm on 3 March", "midterm on 3 March"),
        ("/magnesium:learn\n\nlimits", "limits"),
        // A command alone, another program's command, or a command glued to
        // a word is the learner's own text.
        ("/magnesium:learn", "/magnesium:learn"),
        ("/magnesium:learn   ", "/magnesium:learn"),
        ("/other:learn limits", "/other:learn limits"),
        ("/magnesium:learnlimits", "/magnesium:learnlimits"),
        ("/magnesium:Learn limits", "/magnesium:Learn limits"),
        ("why /magnesium:learn limits", "why /magnesium:learn limits"),
    ] {
        assert_eq!(learner_text(&host, raw), stored, "{raw:?}");
        assert_eq!(learner_text(&host, stored), stored, "{stored:?}");
    }
    let tool_only = HostContext {
        host_kind: "tool_only".into(),
        ..host.clone()
    };
    assert_eq!(learner_text(&tool_only, opener), opener);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn claude_paste_markup_is_certified_and_stored_as_the_learner_wrote_it() {
    let (root, workspace, host) = fixture();
    let words = "Why is E(w, b) a bowl?\n$$X^\\top X w = X^\\top t$$";
    // A whole-message paste from the Claude desktop app, byte for byte.
    let pasted =
        format!("\n\n<pasted_content id=\"b185\">\n{words}\n</pasted_content id=\"b185\">\n");
    let ticket = issue(&workspace, &host, "turn-1", &pasted, 1_000).unwrap();
    // The tutor may echo the learner's words alone or the whole prompt.
    for echo in [words, pasted.as_str()] {
        let event = verify(&workspace, &host, &ticket, echo, 1_500).unwrap();
        assert_eq!(event.text, words);
    }
    assert_eq!(
        verify(&workspace, &host, &ticket, "Why is E(w, b) a bowl?", 1_500).err(),
        Some(failure())
    );

    for (raw, stored) in [
        (
            "Also\n\n<pasted_content id=\"9bc0\">\nX = 1\n</pasted_content id=\"9bc0\">\n\nwhy?",
            "Also\n\nX = 1\n\nwhy?",
        ),
        (
            "<pasted_content id=\"a\">\n<pasted_content id=\"b\">\nx\n</pasted_content id=\"b\">\n</pasted_content id=\"a\">",
            "x",
        ),
        (
            "<pasted_content id=\"a\">\n</pasted_content id=\"a\">\nok\n",
            "ok",
        ),
        // Pasted code keeps its first line's indentation.
        (
            "\n\n<pasted_content id=\"c\">\n    if x:\n        return x\n</pasted_content id=\"c\">\n",
            "    if x:\n        return x",
        ),
        ("  indented\r\n\r\n", "  indented"),
        // Unmatched, inline or malformed tags are the learner's own text.
        (
            "<pasted_content id=\"a\">\nx\n</pasted_content id=\"b\">",
            "<pasted_content id=\"a\">\nx\n</pasted_content id=\"b\">",
        ),
        (
            "see <pasted_content id=\"a\">\nx\n</pasted_content id=\"a\">",
            "see <pasted_content id=\"a\">\nx\n</pasted_content id=\"a\">",
        ),
        (
            "<pasted_content id=\"a b\">\nx\n</pasted_content id=\"a b\">",
            "<pasted_content id=\"a b\">\nx\n</pasted_content id=\"a b\">",
        ),
    ] {
        assert_eq!(learner_text(&host, raw), stored);
        assert_eq!(learner_text(&host, stored), stored);
    }
    let tool_only = HostContext {
        host_kind: "tool_only".into(),
        ..host.clone()
    };
    assert_eq!(learner_text(&tool_only, &pasted), pasted.as_str());
    fs::remove_dir_all(root).unwrap();
}
