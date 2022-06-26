use anyhow::{anyhow, Context};
use rustpython::vm::py_freeze;
use rustpython_vm as vm;
use rustpython_vm::bytecode::frozen_lib::FrozenModulesIter;
use rustpython_vm::{Interpreter, Settings};

fn main() {
    let interp = Interpreter::with_init(Settings::default(), |vm| {
        vm.add_native_modules(rustpython_stdlib::get_module_inits());
        let frozens: FrozenModulesIter = py_freeze!(dir = "pytube/pytube");

        let frozens = frozens.map(|(name, module)| {
            let name = if name.is_empty() {
                "pytube".to_string()
            } else {
                format!("pytube.{}", name)
            };
            println!("frozen: {}", name);
            (name, module)
        });

        vm.add_frozen(frozens);
    });

    interp
        .enter(|vm| -> anyhow::Result<()> {
            let scope = vm.new_scope_with_builtins();

            let code_obj = vm
                .compile(
                    r#"
from time import time

s = time()
print('importing pytube...')
import pytube
e = time()
print("pytube imported in %f" % (e - s))

from pytube import YouTube

print("downloading...")
s = time()
streams = YouTube('https://www.youtube.com/watch?v=5DDw-0ghJR4').streams
print(streams)
e = time()
print("downloaded in %f" % (e - s))

print("downloading... (again)")
s = time()
streams = YouTube('https://www.youtube.com/watch?v=5DDw-0ghJR4').streams
print(streams)
e = time()
print("downloaded in %f" % (e - s))

"#,
                    vm::compile::Mode::Exec,
                    "<embedded>".to_owned(),
                )
                .context("compiling the code")?;

            vm.run_code_obj(code_obj, scope)
                .map_err(|e| {
                    let mut s = String::new();

                    vm.write_exception(&mut s, &e).unwrap();

                    anyhow!("{}", s)
                })
                .context("Running the code")?;

            Ok(())
        })
        .unwrap();
}
