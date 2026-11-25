#!/usr/bin/env python3
"""Proves how tools/gate.py weighs a test program when it shares programs out
over shards: a test target whose source is a `main.rs` weighs the tests of its
whole folder (decision 0446), as a library does, and any other test target
weighs the tests of its own file.

And proves the check that CI's shards are one run of the workspace's tests
(decision 0449): it reads cargo's lists, it names a test left out, run twice,
or not in the full list, and, on a small workspace built here with cargo, the
shards' records of a real run match the full list cargo gives.

And proves the nightly search for flaky tests, `tools/flaky.py` (decision
0999): it runs every program once and the focus programs again, within its
time, names each failed test with its pass, and runs a failed program no more
times than it would a passing one.

And proves the lints for Windows and macOS (decision 0999): every run on Linux
holds them, and the stand-ins they give the C dependencies' build scripts are
refused by the gate for any step but `cargo clippy`, and refuse by themselves
to run outside a `cargo check`, so that no build that links or ships can use
them; on a small crate built here with cargo, `cargo check` and `cargo clippy`
pass with them and `cargo build` and `cargo test` are refused."""
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE / "cross"))
import flaky  # noqa: E402
import gate  # noqa: E402
import lintonly  # noqa: E402


def folder_with(files):
    root = Path(tempfile.mkdtemp(prefix="a2m-gate-"))
    for name, text in files.items():
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(text, encoding="utf-8")
    return root


TWO = "#[test]\nfn a() {}\n#[tokio::test]\nasync fn b() {}\n"
ONE = "#[test]\nfn c() {}\n"


class Weights(unittest.TestCase):
    def test_a_main_rs_test_target_weighs_its_whole_folder(self):
        root = folder_with({"tests/main.rs": "mod write;\nmod read;\n", "tests/write.rs": TWO, "tests/read.rs": ONE})
        self.assertEqual(gate.program_weight(["test"], str(root / "tests" / "main.rs")), 3)

    def test_a_test_target_of_one_file_weighs_that_file_alone(self):
        root = folder_with({"tests/write.rs": TWO, "tests/read.rs": ONE})
        self.assertEqual(gate.program_weight(["test"], str(root / "tests" / "read.rs")), 1)

    def test_a_library_weighs_its_whole_folder(self):
        root = folder_with({"src/lib.rs": ONE, "src/store.rs": TWO})
        self.assertEqual(gate.program_weight(["lib"], str(root / "src" / "lib.rs")), 3)

    def test_a_source_that_cannot_be_read_weighs_nothing(self):
        root = folder_with({})
        self.assertEqual(gate.program_weight(["test"], str(root / "gone.rs")), 0)


CARGO_LISTING = """\
   Compiling alpha v0.1.0 (/w/alpha)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.00s
     Running unittests src/lib.rs (target/debug/deps/alpha-0a1b2c3d)
tests::adds: test
tests::slow: test
     Running tests/one.rs (target\\debug\\deps\\one-99ffee00.exe)
\x1b[1mwrites: test\x1b[0m
   Doc-tests alpha
src/lib.rs - add (line 3): test
"""


class Records(unittest.TestCase):
    def test_cargos_listing_names_each_program_and_test(self):
        self.assertEqual(gate.parse_listing(CARGO_LISTING), [
            ("alpha-0a1b2c3d", "tests::adds"),
            ("alpha-0a1b2c3d", "tests::slow"),
            ("one-99ffee00", "writes"),
            ("doctests alpha", "src/lib.rs - add (line 3)"),
        ])

    def test_a_package_is_named_from_its_id_with_or_without_a_name_in_it(self):
        self.assertEqual(gate.package_name("path+file:///w/crates/a2m-core#0.0.1"), "a2m-core")
        self.assertEqual(gate.package_name("path+file:///w/tools/field-harness#a2m-field-harness@0.0.1"), "a2m-field-harness")
        self.assertEqual(gate.package_name("registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0"), "serde")

    def test_a_program_is_known_by_its_file_name_on_every_system(self):
        self.assertEqual(gate.program_key("C:\\a\\target\\debug\\deps\\one-99.exe"), "one-99")
        self.assertEqual(gate.program_key("/w/target/debug/deps/one-99"), "one-99")

    def test_shards_that_ran_every_test_once_match(self):
        full = [("p", "a"), ("p", "b"), ("q", "a")]
        self.assertEqual(gate.compare_tests(full, [[("p", "a"), ("q", "a")], [("p", "b")]]), [])

    def test_a_test_no_shard_ran_is_named(self):
        full = [("p", "a"), ("p", "b")]
        self.assertEqual(gate.compare_tests(full, [[("p", "a")]]), ["missing: p b"])

    def test_a_test_two_shards_ran_is_named(self):
        full = [("p", "a")]
        self.assertEqual(gate.compare_tests(full, [[("p", "a")], [("p", "a")]]), ["run 2 times: p a"])

    def test_a_test_the_full_list_has_not_got_is_named(self):
        full = [("p", "a")]
        self.assertEqual(gate.compare_tests(full, [[("p", "a"), ("r", "z")]]), ["not in the full list: r z"])

    def test_an_empty_full_list_never_matches(self):
        self.assertEqual(gate.compare_tests([], [[]]), ["the full list holds no test"])

    def test_a_missing_record_fails_the_comparison(self):
        root = folder_with({"full.txt": "p\ta\n"})
        self.assertEqual(gate.compare_main([str(root / "full.txt"), str(root / "shard-1.txt")]), 1)


WORKSPACE = {
    "Cargo.toml": '[workspace]\nmembers = ["alpha", "beta"]\nresolver = "2"\n',
    "alpha/Cargo.toml": '[package]\nname = "alpha"\nversion = "0.1.0"\nedition = "2021"\n',
    "alpha/src/lib.rs": "/// ```\n/// assert_eq!(alpha::add(1, 1), 2);\n/// ```\npub fn add(a: u8, b: u8) -> u8 { a + b }\n"
    "#[cfg(test)]\nmod tests {\n#[test]\nfn adds() { assert_eq!(super::add(2, 2), 4); }\n#[test]\nfn dir() { assert!(std::path::Path::new(\"Cargo.toml\").exists()); }\n}\n",
    "alpha/tests/whole/main.rs": "mod read;\n#[test]\nfn whole() {}\n",
    "alpha/tests/whole/read.rs": "#[test]\nfn reads() {}\n#[test]\n#[ignore]\nfn slow() {}\n",
    "alpha/tests/one.rs": "#[test]\nfn one_of_alpha() {}\n",
    "beta/Cargo.toml": '[package]\nname = "beta"\nversion = "0.1.0"\nedition = "2021"\n',
    "beta/src/lib.rs": "#[test]\nfn b1() {}\n#[test]\nfn b2() {}\n",
    "beta/tests/one.rs": "#[test]\nfn one_of_beta() {}\n",
}


class OneRun(unittest.TestCase):
    """The shards of a real build, run as CI runs them, are the one run cargo
    lists: two programs named `one` in two packages are told apart, the
    doctests are run once, and an ignored test is listed as cargo lists it."""

    def test_the_shards_of_a_real_workspace_match_its_full_list(self):
        if shutil.which("cargo") is None:
            self.fail("cargo is needed to prove the shards of a real build")
        root = folder_with(WORKSPACE)
        env = {**os.environ, "CARGO_TARGET_DIR": str(root / "target"), "CARGO_TERM_COLOR": "always"}
        env.pop("RUSTUP_TOOLCHAIN", None)
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, env=env, check=True, capture_output=True)
        here = os.getcwd()
        os.chdir(root)
        try:
            self.assertEqual(gate.list_tests(str(root / "full.txt"), env), 0)
            records = []
            for k in (1, 2):
                ok, _, output, failed, ran = gate.shard_tests(k, 2, env)
                self.assertTrue(ok, f"{failed}\n{output}")
                gate.write_record(str(root / f"shard-{k}.txt"), ran)
                records.append(str(root / f"shard-{k}.txt"))
        finally:
            os.chdir(here)
        self.addCleanup(shutil.rmtree, root, ignore_errors=True)
        full = gate.read_record(str(root / "full.txt"))
        self.assertEqual(len(full), 10, full)
        self.assertEqual(len({program for program, _ in full}), 6, full)
        self.assertEqual(gate.compare_main([str(root / "full.txt")] + records), 0)
        self.assertNotEqual(gate.compare_main([str(root / "full.txt"), records[0]]), 0)
        self.assertNotEqual(gate.compare_main([str(root / "full.txt")] + records + records[:1]), 0)


class CrossPlan(unittest.TestCase):
    def names(self, argv, platform="linux"):
        return [name for name, _, _ in gate.plan(argv, platform)]

    def test_every_run_on_linux_lints_for_windows_and_macos_before_the_tests(self):
        cross = ["cross targets", "lint for x86_64-pc-windows-msvc", "lint for aarch64-apple-darwin"]
        for argv in ([], ["--fast"], ["--shard", "1/3"]):
            names = self.names(argv)
            self.assertEqual(names[names.index("lint") + 1:names.index("lint") + 4], cross, argv)
        self.assertEqual(self.names([])[-1], "test")

    def test_the_shards_that_run_tests_alone_lint_nothing(self):
        self.assertEqual(self.names(["--shard", "2/3", "--tests-only"]), ["toolchain", "test"])
        self.assertEqual(self.names(["--shard", "1/3", "--tests-only"]), ["toolchain", "test"])
        self.assertEqual(self.names(["--shard", "2/3"]), ["toolchain", "test"])

    def test_off_linux_the_cross_lints_are_left_out(self):
        self.assertNotIn("cross targets", self.names([], "darwin"))
        self.assertIn("lint", self.names([], "win32"))

    def test_the_cross_lints_are_clippy_with_warnings_denied_and_the_targets_added(self):
        steps = gate.cross_steps()
        self.assertEqual(steps[0][1], ["rustup", "target", "add"] + gate.CROSS_TARGETS)
        for _, command, extra in steps[1:]:
            self.assertEqual(command[:3], ["cargo", "clippy", "--workspace"])
            self.assertEqual(command[-3:], ["--", "-D", "warnings"])
            self.assertEqual(extra[gate.LINT_ONLY], "1")
            self.assertIsNone(gate.refused(command, extra))

    def test_the_gate_refuses_to_give_the_stand_ins_to_a_build(self):
        _, _, extra = gate.cross_steps()[1]
        for verb in ("build", "test", "run", "install", "check"):
            self.assertIsNotNone(gate.refused(["cargo", verb, "--workspace"], extra), verb)
        stand_in_alone = {"CC_x": str(HERE / "cross" / "nocc.py")}
        self.assertIsNotNone(gate.refused(["cargo", "build"], stand_in_alone))
        self.assertIsNone(gate.refused(["cargo", "build"], {}))
        ok, _, output, _ = gate.run("build", ["cargo", "build"], {**os.environ, **extra}, extra)
        self.assertFalse(ok)
        self.assertIn("not `cargo clippy`", output)


    def test_no_workflow_gives_a_build_the_stand_ins(self):
        for workflow in sorted((HERE.parent / ".github" / "workflows").glob("*.yml")):
            text = workflow.read_text(encoding="utf-8")
            for name in ("tools/cross", "nocc.py", "noar.py", gate.LINT_ONLY):
                self.assertNotIn(name, text, workflow.name)


class LintOnlyGuard(unittest.TestCase):
    ON = {lintonly.MARKER: "1"}

    def test_a_cargo_check_above_is_allowed(self):
        chain = [["/w/target/debug/build/x/build-script-build"], ["/t/bin/cargo", "check", "--target", "t"], ["/t/bin/cargo-clippy", "clippy"]]
        self.assertIsNone(lintonly.why_refused(self.ON, chain))
        self.assertIsNone(lintonly.why_refused(self.ON, [["cargo", "+stable", "-q", "check"]]))

    def test_any_other_cargo_command_above_is_refused(self):
        for verb in ("build", "test", "run", "install", "rustc", "publish", "package", "bench", "doc"):
            reason = lintonly.why_refused(self.ON, [["build-script-build"], ["/t/bin/cargo", verb], ["cargo", "check"]])
            self.assertIn(f"`cargo {verb}`", reason or "", verb)

    def test_no_cargo_above_is_refused(self):
        self.assertIsNotNone(lintonly.why_refused(self.ON, [["python3"], ["bash"]]))

    def test_without_the_marker_it_is_refused(self):
        self.assertIsNotNone(lintonly.why_refused({}, [["cargo", "check"]]))
        self.assertIsNotNone(lintonly.why_refused({lintonly.MARKER: "yes"}, [["cargo", "check"]]))

    @unittest.skipUnless(sys.platform.startswith("linux"), "the stand-ins run on Linux alone")
    def test_a_stand_in_run_outside_cargo_writes_nothing(self):
        root = folder_with({})
        self.addCleanup(shutil.rmtree, root, ignore_errors=True)
        env = {**os.environ, **self.ON}
        cc = subprocess.run([sys.executable, str(HERE / "cross" / "nocc.py"), "-c", "x.c", "-o", str(root / "x.o")], env=env, capture_output=True, text=True)
        ar = subprocess.run([sys.executable, str(HERE / "cross" / "noar.py"), "cq", str(root / "x.a"), str(root / "x.o")], env=env, capture_output=True, text=True)
        self.assertEqual((cc.returncode, ar.returncode), (3, 3), cc.stderr + ar.stderr)
        self.assertIn("no cargo was found", cc.stderr)
        self.assertEqual(list(root.iterdir()), [])


BUILD_RS = r"""
use std::{env, path::PathBuf, process::Command};
fn main() {
    let target = env::var("TARGET").unwrap().replace('-', "_");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let cc = env::var(format!("CC_{target}")).unwrap();
    let ar = env::var(format!("AR_{target}")).unwrap();
    let object = out.join("x.o");
    let archive = out.join("libx.a");
    assert!(Command::new(&cc).args(["-c", "x.c", "-o"]).arg(&object).status().unwrap().success(), "the stand-in compiler refused");
    assert!(Command::new(&ar).arg("cq").arg(&archive).arg(&object).status().unwrap().success(), "the stand-in archiver refused");
    assert!(object.exists() && archive.exists());
}
"""


@unittest.skipUnless(sys.platform.startswith("linux"), "the stand-ins run on Linux alone")
class LintOnlyStandInsInCargo(unittest.TestCase):
    """A build script given the stand-ins, as a C dependency's is, passes in
    `cargo check` and `cargo clippy` and fails in `cargo build` and `cargo test`."""

    def test_the_stand_ins_serve_a_check_and_refuse_a_build(self):
        if shutil.which("cargo") is None:
            self.fail("cargo is needed to prove the stand-ins in a real build")
        root = folder_with({
            "Cargo.toml": '[package]\nname = "standin"\nversion = "0.1.0"\nedition = "2021"\n',
            "src/lib.rs": "#[test]\nfn t() {}\n",
            "build.rs": BUILD_RS,
        })
        self.addCleanup(shutil.rmtree, root, ignore_errors=True)
        host = re.search(r"^host: (\S+)$", subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout, re.M).group(1)
        under = host.replace("-", "_")
        env = {**os.environ, "CARGO_TARGET_DIR": str(root / "target"), "CARGO_TERM_COLOR": "never",
               f"CC_{under}": str(HERE / "cross" / "nocc.py"), f"AR_{under}": str(HERE / "cross" / "noar.py"), lintonly.MARKER: "1"}
        env.pop("RUSTUP_TOOLCHAIN", None)
        for verb in ("check", "clippy"):
            done = subprocess.run(["cargo", verb, "--offline"], cwd=root, env=env, capture_output=True, text=True)
            self.assertEqual(done.returncode, 0, f"cargo {verb}:\n{done.stderr}")
        for verb in ("build", "test"):
            subprocess.run(["cargo", "clean"], cwd=root, env=env, capture_output=True)
            done = subprocess.run(["cargo", verb, "--offline"], cwd=root, env=env, capture_output=True, text=True)
            self.assertNotEqual(done.returncode, 0, f"cargo {verb} passed with the stand-ins")
            self.assertIn(f"refused: the cargo that runs this is `cargo {verb}`", done.stderr)


class Flaky(unittest.TestCase):
    P = [(1, "a lib a", "/a", "/w/a", "a"), (1, "b lib b", "/b", "/w/b", "b")]

    def test_the_focus_programs_run_first_and_then_again(self):
        runs = flaky.schedule(self.P + [(1, "c lib c", "/c", "/w/c", "c")], {"b"}, 3, 0)
        self.assertEqual([(n, f, p[1]) for n, f, p in runs], [
            (1, True, "b lib b"), (1, False, "a lib a"), (1, False, "c lib c"),
            (2, True, "b lib b"), (3, True, "b lib b"), (4, True, "b lib b")])

    def test_the_other_programs_start_from_a_different_one_each_day(self):
        three = self.P + [(1, "c lib c", "/c", "/w/c", "c")]
        firsts = {flaky.schedule(three, set(), 0, day)[0][2][1] for day in range(3)}
        self.assertEqual(firsts, {"a lib a", "b lib b", "c lib c"})

    def test_what_may_start_in_the_time_left(self):
        self.assertTrue(flaky.fits(1, True, 999, 10_000, 60))
        self.assertTrue(flaky.fits(1, False, 0, 59, 60))
        self.assertFalse(flaky.fits(1, False, 0, 60, 60))
        self.assertTrue(flaky.fits(2, True, 20, 30, 60))
        self.assertFalse(flaky.fits(2, True, 40, 30, 60))

    def test_the_failed_tests_are_named(self):
        output = ("test a::ok ... ok\ntest a::races ... FAILED\n\nfailures:\n\n---- a::races stdout ----\npanicked\n\n"
                  "failures:\n    a::races\n    a::hangs\n\ntest result: FAILED. 1 passed; 2 failed\n")
        self.assertEqual(flaky.failed_tests(output), ["a::hangs", "a::races"])
        self.assertEqual(flaky.failed_tests("test result: ok. 3 passed\n"), [])


FLAKY_WORKSPACE = {
    "Cargo.toml": '[workspace]\nmembers = ["steady", "racy"]\nresolver = "2"\n',
    "steady/Cargo.toml": '[package]\nname = "steady"\nversion = "0.1.0"\nedition = "2021"\n',
    "steady/src/lib.rs": "#[test]\nfn holds() {}\n",
    "racy/Cargo.toml": '[package]\nname = "racy"\nversion = "0.1.0"\nedition = "2021"\n',
    # Fails on its second run alone, as a race does now and then: the run
    # that fails is never repeated to pass, and the third run still happens.
    "racy/src/lib.rs": "#[test]\nfn second_run_fails() {\n"
    "    let path = std::env::var(\"A2M_FLAKY_COUNT\").unwrap();\n"
    "    let before = std::fs::read_to_string(&path).unwrap_or_default();\n"
    "    std::fs::write(&path, format!(\"{before}x\")).unwrap();\n"
    "    assert_ne!(before.len(), 1, \"the second run fails\");\n}\n"
    "#[test]\nfn steady_too() {}\n",
}


class FlakyRun(unittest.TestCase):
    def test_a_test_that_fails_once_in_three_runs_is_named_and_not_retried(self):
        if shutil.which("cargo") is None:
            self.fail("cargo is needed to prove the search for flaky tests")
        root = folder_with(FLAKY_WORKSPACE)
        self.addCleanup(shutil.rmtree, root, ignore_errors=True)
        env = {**os.environ, "CARGO_TARGET_DIR": str(root / "target"), "A2M_FLAKY_COUNT": str(root / "count")}
        env.pop("RUSTUP_TOOLCHAIN", None)
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, env=env, check=True, capture_output=True)
        done = subprocess.run([sys.executable, str(HERE / "flaky.py"), "--load", "1", "--repeat", "2", "--focus", "racy"],
                              cwd=root, env=env, capture_output=True, text=True, timeout=600)
        self.assertEqual(done.returncode, 1, done.stdout + done.stderr)
        self.assertIn("FAILED pass 2: racy lib racy (exit 101): second_run_fails", done.stdout)
        self.assertEqual(done.stdout.count("FAILED pass"), 1, done.stdout)
        self.assertEqual((root / "count").read_text(), "xxx")
        self.assertEqual(len(re.findall(r"^  pass \d+ .*steady lib steady$", done.stdout, re.M)), 1, done.stdout)
        self.assertEqual(len(re.findall(r"^  pass \d+ .*racy lib racy$", done.stdout, re.M)), 3, done.stdout)

    def test_a_steady_workspace_passes(self):
        if shutil.which("cargo") is None:
            self.fail("cargo is needed to prove the search for flaky tests")
        root = folder_with({k: v for k, v in FLAKY_WORKSPACE.items() if not k.startswith("racy")} | {"Cargo.toml": '[workspace]\nmembers = ["steady"]\nresolver = "2"\n'})
        self.addCleanup(shutil.rmtree, root, ignore_errors=True)
        env = {**os.environ, "CARGO_TARGET_DIR": str(root / "target")}
        env.pop("RUSTUP_TOOLCHAIN", None)
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, env=env, check=True, capture_output=True)
        done = subprocess.run([sys.executable, str(HERE / "flaky.py"), "--load", "1", "--repeat", "1", "--focus", "steady"],
                              cwd=root, env=env, capture_output=True, text=True, timeout=600)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)
        self.assertIn("2 runs", done.stdout)
        self.assertIn("0 failed", done.stdout)


if __name__ == "__main__":
    unittest.main()
