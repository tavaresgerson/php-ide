//! Phase 6 gate: run configurations, the Run tool window, PHPUnit tests
//! (tree, rerun failed, coverage), output links and a debug session, all
//! driven through the UI.
//!
//! PHPUnit is a small PHP script in `vendor/bin/phpunit` that speaks the
//! TeamCity protocol and writes Clover coverage, so the gate runs without
//! Composer or network; the debugger side is a DBGp peer that answers like
//! Xdebug (the extension is not needed).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use crate::ui_tests::{harness, step_until};
use crate::PhpIdeApp;

const FAKE_PHPUNIT: &str = r###"<?php
$filter = null;
$clover = null;
for ($i = 1; $i < count($argv); $i++) {
    if ($argv[$i] === '--filter') { $filter = $argv[++$i]; }
    elseif ($argv[$i] === '--coverage-clover') { $clover = $argv[++$i]; }
}
$root = getcwd();
$file = $root . '/tests/CalcTest.php';
$tests = ['testAdds' => null, 'testFails' => 'Failed asserting that 3 is identical to 4.'];
$selected = [];
foreach ($tests as $name => $fail) {
    if ($filter === null || preg_match('{' . $filter . '}', 'Tests\\CalcTest::' . $name)) {
        $selected[$name] = $fail;
    }
}
function tc($s) { return str_replace(['|', "'", "\n", '[', ']'], ['||', "|'", '|n', '|[', '|]'], $s); }
echo "##teamcity[testCount count='" . count($selected) . "' flowId='1']\n";
echo "##teamcity[testSuiteStarted name='Tests\\CalcTest' locationHint='php_qn://" . tc($file) . "::\\Tests\\CalcTest' flowId='1']\n";
foreach ($selected as $name => $fail) {
    echo "##teamcity[testStarted name='$name' locationHint='php_qn://" . tc($file) . "::\\Tests\\CalcTest::$name' flowId='1']\n";
    if ($fail) {
        echo "##teamcity[testFailed name='$name' message='" . tc($fail) . "' details='" . tc($file . ":12\n") . "' flowId='1']\n";
    }
    echo "##teamcity[testFinished name='$name' duration='2' flowId='1']\n";
}
echo "##teamcity[testSuiteFinished name='Tests\\CalcTest' flowId='1']\n";
if ($clover) {
    file_put_contents($clover, '<?xml version="1.0"?><coverage><project><file name="' . $root . '/src/Calc.php"><line num="5" type="stmt" count="1"/><line num="6" type="stmt" count="0"/><line num="7" type="stmt" count="3"/><line num="8" type="stmt" count="1"/></file></project></coverage>');
}
exit(isset($selected['testFails']) ? 1 : 0);
"###;

const CALC_TEST: &str = "<?php\n\nnamespace Tests;\n\nuse PHPUnit\\Framework\\TestCase;\n\nfinal class CalcTest extends TestCase\n{\n    public function testAdds(): void\n    {\n        $this->assertSame(4, 2 + 2);\n    }\n\n    public function testFails(): void\n    {\n        $this->assertSame(4, 3);\n    }\n}\n";

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    for d in ["src", "tests", "vendor/bin", "bin"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    std::fs::write(root.join("vendor/bin/phpunit"), FAKE_PHPUNIT).unwrap();
    std::fs::write(root.join("tests/CalcTest.php"), CALC_TEST).unwrap();
    std::fs::write(
        root.join("src/Calc.php"),
        "<?php\n\nfinal class Calc\n{\n    public function add($a, $b) { return $a + $b; }\n    public function sub($a, $b) { return $a - $b; }\n    public function mul($a, $b) { return $a * $b; }\n    public function div($a, $b) { return $a / $b; }\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("bin/fail.php"),
        "<?php\ntrigger_error('boom', E_USER_WARNING);\necho \"done\\n\";\n",
    )
    .unwrap();
    std::fs::write(
        root.join("bin/debug.php"),
        "<?php\n$count = 3;\n$user = ['name' => 'Ana', 'id' => 7];\necho $count;\n",
    )
    .unwrap();
    std::fs::write(root.join("composer.json"), "{}").unwrap();
    (dir, root)
}

fn php_available() -> bool {
    std::process::Command::new("php")
        .arg("-v")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn wait(
    h: &mut Harness<'static, PhpIdeApp>,
    what: &str,
    done: impl Fn(&Harness<'static, PhpIdeApp>) -> bool,
) {
    step_until(h, what, |h| done(h));
}

fn last_tree(h: &Harness<'static, PhpIdeApp>) -> Option<phpide_run::TestTree> {
    let id = h.state().rd.selected?;
    h.state().rd.test_trees.get(&id).cloned()
}

#[test]
fn run_configurations_tests_rerun_failed_and_coverage() {
    if !php_available() {
        eprintln!("SKIP: php is not installed");
        return;
    }
    let (_dir, root) = project();
    let mut h = harness(&root, Vec::new());
    h.run_steps(3);

    // Run › Edit Configurations: add a PHPUnit configuration, OK.
    let ctx = h.ctx.clone();
    h.state_mut()
        .dispatch_command(crate::commands::Command::EditConfigurations, &ctx);
    h.run_steps(2);
    // With no configuration yet, the list of types is already open.
    h.get_by_label("PHPUnit").click();
    h.run_steps(2);
    assert_eq!(
        h.state().run_dialog.as_ref().map(|d| d.configs.len()),
        Some(1)
    );
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(h.state().run_dialog.is_none());
    let saved = std::fs::read_to_string(root.join(".phpide/run-configurations.json")).unwrap();
    assert!(saved.contains("\"type\": \"php_unit\""), "{saved}");
    assert_eq!(
        h.state().selected_run_config().unwrap().name,
        "Unnamed PHPUnit"
    );

    // Shift+F10 runs it: the tree shows one failure and one pass.
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F10);
    wait(&mut h, "the test tree", |h| last_tree(h).is_some());
    let stats = last_tree(&h).unwrap().stats();
    assert_eq!((stats.total, stats.passed, stats.failed), (2, 1, 1));
    h.run_steps(2);
    assert!(h.query_by_label_contains("Tests failed: 1").is_some());

    // Rerun Failed Tests runs only testFails.
    h.get_by_label("Rerun Failed Tests").click();
    h.run_steps(2);
    wait(&mut h, "the rerun", |h| {
        last_tree(h).is_some_and(|t| t.stats().total == 1)
    });
    let failures = last_tree(&h).unwrap();
    assert_eq!(failures.failures()[0].name, "testFails");

    // Coverage: the gutter data and the Project view percentage.
    h.state_mut()
        .dispatch_command(crate::commands::Command::RunWithCoverage, &ctx);
    wait(&mut h, "coverage", |h| h.state().rd.coverage.is_some());
    let report = h.state().rd.coverage.clone().unwrap();
    let calc = report
        .file(&root.join("src/Calc.php"))
        .expect("Calc.php covered");
    assert_eq!(calc.covered_lines(), 3);
    h.state_mut()
        .tw
        .set_visible(crate::toolwindows::ToolWindow::Project, true);
    {
        let app = h.state_mut();
        app.files
            .reveal(&app.jobs, &ctx, &root.join("src/Calc.php"));
    }
    wait(&mut h, "the tree row", |h| {
        h.query_by_value("75% lines covered").is_some()
    });
}

#[test]
fn context_runs_tests_at_the_caret_and_output_links_open_files() {
    if !php_available() {
        eprintln!("SKIP: php is not installed");
        return;
    }
    let (_dir, root) = project();
    let test_file = root.join("tests/CalcTest.php");
    let mut h = harness(&root, vec![test_file.clone()]);
    h.run_steps(3);

    // Caret inside testAdds: Ctrl+Shift+F10 runs that method only.
    {
        let app = h.state_mut();
        let text = app.ws.active_buffer().unwrap().document.text();
        let at = text[..text.find("2 + 2").unwrap()].chars().count();
        app.editors.set_caret(&mut app.ws, at);
        app.cursor_char = at;
    }
    h.run_steps(1);
    h.key_press_modifiers(
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        egui::Key::F10,
    );
    wait(&mut h, "the context run", |h| last_tree(h).is_some());
    let tree = last_tree(&h).unwrap();
    assert_eq!((tree.stats().total, tree.stats().passed), (1, 1));
    let selected = h.state().selected_run_config().unwrap().clone();
    assert_eq!(selected.name, "CalcTest::testAdds");
    assert!(h.state().rd.temporary.iter().any(|c| c.id == selected.id));

    // A script's warning names its file and line: the link opens it.
    let fail = root.join("bin/fail.php");
    h.state_mut().open_file_tab(&fail, false);
    h.run_steps(2);
    h.key_press_modifiers(
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        egui::Key::F10,
    );
    wait(&mut h, "the script output", |h| {
        let app = h.state();
        app.rd
            .selected
            .and_then(|id| app.rd.manager.session(id))
            .is_some_and(|s| !s.state.is_live() && s.output().contains("on line 2"))
    });
    h.run_steps(2);
    // Back to the test file, then follow the link.
    h.state_mut().open_file_tab(&test_file, false);
    h.run_steps(2);
    assert!(
        h.query_all_by_label_contains("fail.php on line 2")
            .next()
            .is_some(),
        "a link in the output"
    );
    h.get_all_by_label_contains("fail.php on line 2")
        .next()
        .unwrap()
        .click();
    h.run_steps(3);
    let app = h.state();
    let buffer = app.ws.active_buffer().unwrap();
    assert_eq!(buffer.path.as_deref(), Some(fail.as_path()));
}

/// A DBGp peer that answers like Xdebug for the commands the IDE sends.
fn dbgp_peer(port: u16, root: &Path) -> std::thread::JoinHandle<Vec<String>> {
    let file = format!("file://{}", root.join("bin/debug.php").display());
    std::thread::spawn(move || {
        let mut stream = loop {
            if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
                break s;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let send = |stream: &mut TcpStream, xml: String| {
            let _ = stream.write_all(xml.as_bytes());
            let _ = stream.write_all(&[0]);
        };
        send(
            &mut stream,
            format!(
                r#"<init xmlns="urn:debugger_protocol_v1" fileuri="{file}" language="PHP" protocol_version="1.0" appid="1" idekey="phpide"></init>"#
            ),
        );
        let mut reader = stream.try_clone().unwrap();
        let mut seen = Vec::new();
        let mut runs = 0;
        loop {
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                match reader.read(&mut byte) {
                    Ok(0) | Err(_) => return seen,
                    Ok(_) if byte[0] == 0 => break,
                    Ok(_) => buf.push(byte[0]),
                }
            }
            let command = String::from_utf8_lossy(&buf).into_owned();
            let words: Vec<&str> = command.split_whitespace().collect();
            let name = words.first().copied().unwrap_or("");
            let arg = |flag: &str| -> String {
                words
                    .iter()
                    .position(|w| *w == flag)
                    .and_then(|i| words.get(i + 1))
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            };
            let tid = arg("-i");
            seen.push(name.to_string());
            let xml = match name {
                "breakpoint_set" => format!(
                    r#"<response command="breakpoint_set" transaction_id="{tid}" id="1" success="1"></response>"#
                ),
                "run" | "step_over" | "step_into" | "step_out" => {
                    runs += 1;
                    if runs == 1 {
                        format!(
                            r#"<response xmlns:xdebug="urn:debugger_protocol_v1" command="{name}" transaction_id="{tid}" status="break" reason="ok"><xdebug:message filename="{file}" lineno="4"/></response>"#
                        )
                    } else {
                        format!(
                            r#"<response command="{name}" transaction_id="{tid}" status="stopping" reason="ok"></response>"#
                        )
                    }
                }
                "stack_get" => format!(
                    r#"<response command="stack_get" transaction_id="{tid}"><stack where="{{main}}" level="0" type="file" filename="{file}" lineno="4"/></response>"#
                ),
                "context_get" => format!(
                    r#"<response command="context_get" transaction_id="{tid}"><property name="$count" fullname="$count" type="int"><![CDATA[3]]></property><property name="$user" fullname="$user" type="array" numchildren="2" children="1"></property></response>"#
                ),
                "property_get" if arg("-n") == "$user" => format!(
                    r#"<response command="property_get" transaction_id="{tid}"><property name="$user" fullname="$user" type="array" numchildren="2" children="1"><property name="name" fullname="$user['name']" type="string"><![CDATA[Ana]]></property><property name="id" fullname="$user['id']" type="int"><![CDATA[7]]></property></property></response>"#
                ),
                "property_get" => format!(
                    r#"<response command="property_get" transaction_id="{tid}"><property name="$count" fullname="$count" type="int"><![CDATA[3]]></property></response>"#
                ),
                "stop" | "detach" => {
                    send(
                        &mut stream,
                        format!(
                            r#"<response command="{name}" transaction_id="{tid}" status="stopped" reason="ok"></response>"#
                        ),
                    );
                    return seen;
                }
                _ => format!(
                    r#"<response command="{name}" transaction_id="{tid}" success="1"></response>"#
                ),
            };
            send(&mut stream, xml);
        }
    })
}

#[test]
fn debug_session_frames_variables_evaluate_and_breakpoints() {
    if !php_available() {
        eprintln!("SKIP: php is not installed");
        return;
    }
    let (_dir, root) = project();
    let script = root.join("bin/debug.php");
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    std::env::set_var("PHPIDE_DBGP_PORT", port.to_string());
    let mut h = harness(&root, vec![script.clone()]);
    h.run_steps(3);

    // A breakpoint on line 4 (F9 on the caret's line), then Shift+F9.
    {
        let app = h.state_mut();
        let text = app.ws.active_buffer().unwrap().document.text();
        let at = text[..text.find("echo").unwrap()].chars().count();
        app.editors.set_caret(&mut app.ws, at);
        app.cursor_char = at;
        app.editors.request_focus();
    }
    h.run_steps(2);
    h.key_press(egui::Key::F9);
    h.run_steps(1);
    assert_eq!(
        h.state()
            .rd
            .bp_lines_for(&script)
            .into_iter()
            .collect::<Vec<_>>(),
        vec![4]
    );
    let peer = dbgp_peer(port, &root);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F9);
    wait(&mut h, "the pause", |h| {
        h.state().rd.status == crate::run_debug::DebugStatus::Paused
            && !h.state().rd.variables.is_empty()
    });
    h.run_steps(2);
    assert!(h.query_by_label_contains("Paused at debug.php:4").is_some());
    assert!(h.query_by_label("$count = 3").is_some());

    // Expand $user: its children come lazily.
    h.state_mut().rd.toggle_variable("$user");
    wait(&mut h, "the children", |h| {
        h.state().rd.children.contains_key("$user")
    });
    h.run_steps(2);
    assert!(h.query_by_label("name = \"Ana\"").is_some());

    // Alt+F8: Evaluate Expression.
    h.key_press_modifiers(egui::Modifiers::ALT, egui::Key::F8);
    h.run_steps(2);
    assert!(h.state().rd.evaluate_open);
    for c in "$count".chars() {
        h.event(egui::Event::Text(c.to_string()));
    }
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    wait(&mut h, "the evaluation", |h| {
        h.state().rd.eval_result.is_some()
    });
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!h.state().rd.evaluate_open);

    // Ctrl+Shift+F8: a condition on the breakpoint.
    h.key_press_modifiers(
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        egui::Key::F8,
    );
    h.run_steps(2);
    assert!(h.state().rd.breakpoints_dialog.is_some());
    h.get_by_label("Done").click();
    h.run_steps(2);
    h.state()
        .rd
        .bp_store
        .lock()
        .unwrap()
        .set_condition(&script, 4, "$count > 1".into());

    // F8 steps; the program ends and the session closes.
    h.state_mut().editors.request_focus();
    h.run_steps(2);
    h.key_press(egui::Key::F8);
    wait(&mut h, "the end of the session", |h| {
        h.state().rd.status == crate::run_debug::DebugStatus::Idle
    });
    let seen = peer.join().unwrap();
    assert!(seen.iter().any(|c| c == "property_get"), "{seen:?}");
    assert!(seen.iter().any(|c| c == "step_over"), "{seen:?}");
}

/// Writes the Phase 6 screenshots to `target/snapshots` in the three
/// themes: `cargo test -p phpide-ui phase6_screenshots -- --ignored`.
#[test]
#[ignore]
fn phase6_screenshots() {
    use crate::design::{install, snapshot, Density, Theme, ThemeId};
    if !php_available() {
        eprintln!("SKIP: php is not installed");
        return;
    }
    let (_dir, root) = project();
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    std::env::set_var("PHPIDE_DBGP_PORT", port.to_string());
    let test_file = root.join("tests/CalcTest.php");
    let mut h = harness(&root, vec![test_file]);
    h.run_steps(3);
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let shoot = |h: &mut Harness<'static, PhpIdeApp>, name: &str| {
        for (theme, label) in [
            (ThemeId::Dark, "dark"),
            (ThemeId::Light, "light"),
            (ThemeId::HighContrast, "high-contrast"),
        ] {
            let ctx = egui::Context::default();
            install(&ctx, Theme::new(theme, Density::Comfortable));
            h.state_mut().editors.clear_layouts();
            let image = snapshot::render(&ctx, egui::vec2(1280.0, 800.0), 12, |ui| {
                h.state_mut().frame_ui(ui)
            });
            image.write_png(&out.join(format!("phase6-{name}-{label}.png")));
        }
        h.state_mut().editors.clear_layouts();
    };
    let ctx = h.ctx.clone();

    // The test class at the caret: one failure, one pass.
    {
        let app = h.state_mut();
        let text = app.ws.active_buffer().unwrap().document.text();
        let at = text[..text.find("final class").unwrap()].chars().count() + 8;
        app.editors.set_caret(&mut app.ws, at);
        app.cursor_char = at;
    }
    h.run_steps(1);
    h.key_press_modifiers(
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        egui::Key::F10,
    );
    wait(&mut h, "the test tree", |h| {
        last_tree(h).is_some_and(|t| t.stats().total == 2)
    });
    wait(&mut h, "the end of the run", |h| {
        let app = h.state();
        app.rd
            .selected
            .and_then(|id| app.rd.manager.session(id))
            .is_some_and(|s| !s.state.is_live())
    });
    h.run_steps(3);
    shoot(&mut h, "run-tests");

    // Run/Debug Configurations with a new PHPUnit configuration.
    h.state_mut()
        .dispatch_command(crate::commands::Command::EditConfigurations, &ctx);
    h.run_steps(2);
    if h.query_by_label("PHPUnit").is_none() {
        h.get_by_label("Add New Configuration").click();
        h.run_steps(2);
    }
    h.get_by_label("PHPUnit").click();
    h.run_steps(3);
    shoot(&mut h, "edit-configurations");
    h.get_by_label("Cancel").click();
    h.run_steps(2);

    // The terminal, after a command.
    h.state_mut().dispatch_command(
        crate::commands::Command::ToggleToolWindow(crate::toolwindows::ToolWindow::Terminal),
        &ctx,
    );
    h.run_steps(3);
    wait(&mut h, "the shell", |h| {
        h.state()
            .terminal
            .sessions
            .first()
            .is_some_and(|s| !s.screen_text().trim().is_empty())
    });
    h.state_mut().terminal.sessions[0].write(b"ls -la src tests\r");
    wait(&mut h, "the listing", |h| {
        h.state().terminal.sessions[0]
            .screen_text()
            .contains("Calc.php")
    });
    h.run_steps(3);
    shoot(&mut h, "terminal");

    // A debug session paused on a breakpoint, with $user expanded.
    h.state_mut()
        .tw
        .set_visible(crate::toolwindows::ToolWindow::Terminal, false);
    h.state_mut()
        .tw
        .set_visible(crate::toolwindows::ToolWindow::Run, false);
    let script = root.join("bin/debug.php");
    h.state_mut().open_file_tab(&script, false);
    h.run_steps(2);
    {
        let app = h.state_mut();
        let text = app.ws.active_buffer().unwrap().document.text();
        let at = text[..text.find("echo").unwrap()].chars().count();
        app.editors.set_caret(&mut app.ws, at);
        app.cursor_char = at;
        app.editors.request_focus();
        app.rd.selected_config_id = None;
    }
    h.run_steps(2);
    h.key_press(egui::Key::F9);
    h.run_steps(1);
    let _peer = dbgp_peer(port, &root);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::F9);
    wait(&mut h, "the pause", |h| {
        h.state().rd.status == crate::run_debug::DebugStatus::Paused
            && !h.state().rd.variables.is_empty()
    });
    h.state_mut().rd.toggle_variable("$user");
    wait(&mut h, "the children", |h| {
        h.state().rd.children.contains_key("$user")
    });
    h.run_steps(3);
    shoot(&mut h, "debug");
}
