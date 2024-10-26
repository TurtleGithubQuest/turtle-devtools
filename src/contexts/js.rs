use crate::contexts::{BuildContext, BaseContext};
use std::path::Path;
use serde::Deserialize;
use crate::misc::errors::common::CommonError;
use crate::misc::errors::transfer;

use swc_common::{Globals, SourceMap, FileName};
use swc_bundler::{Bundler, Load, ModuleData, ModuleRecord, Resolve, Hook};
use swc_ecma_parser::{EsConfig, Syntax, JscTarget, parse_file_as_module, EsSyntax};
use swc_ecma_codegen::{Emitter, text_writer::JsWriter, Config as CodegenConfig};
use std::sync::Arc;
use swc_common::errors::Handler;
use swc_common::sync::Lrc;
use swc_ecma_ast::EsVersion;
use swc_ecma_loader::resolve::Resolution;

#[derive(Deserialize, Clone)]
pub struct Context {
    #[serde(flatten)]
    base: BaseContext,
}

impl BuildContext for Context {
    fn context_name(&self) -> &str {
        "js"
    }

    fn base(&self) -> &BaseContext {
        &self.base
    }

    fn build(&self, changed_file: Option<&Path>) -> Result<(), CommonError> {
        let output_folder = self.get_output_folder()?;

        // Ensure the output folder exists
        if !output_folder.exists() {
            std::fs::create_dir_all(&output_folder).map_err(transfer::TransferError::IoError)?;
        }

        // Build JavaScript files
        if let Some(changed_file) = changed_file {
            self.build_file(changed_file, &output_folder)?;
        } else {
            for entrypoint in self.get_entrypoints() {
                let entrypoint_path = Path::new(&entrypoint.entrypoint);
                self.build_file(entrypoint_path, &output_folder)?;
            }
        }
        Ok(())
    }
}

impl Context {
    fn build_file(&self, js_path: &Path, output_folder: &Path) -> Result<(), CommonError> {
        if !js_path.exists() {
            return Err(CommonError::Error(format!("JavaScript file {:?} does not exist.", js_path)));
        }

        // Prepare SWC components
        let cm: Lrc<SourceMap> = Default::default();
        let handler = Handler::with_tty_emitter(
            swc_common::errors::ColorConfig::Auto,
            true,
            false,
            Some(cm),
        );

        let globals = Globals::new();

        // Implement the Loader trait to load modules
        struct FsLoader {
            cm: alloc::rc::Rc<SourceMap>,
            handler: Handler,
        }

        impl Load for FsLoader {
            fn load(&self, file_name: &FileName) -> Result<ModuleData, anyhow::Error> {
                let path = match &file_name {
                    FileName::Real(path) => path.clone(),
                    _ => panic!("Expected real file name, got {:?}", file_name),
                };

                let fm = self.cm.load_file(&path)?;
                let mut errors = vec![];

                let module = parse_file_as_module(
                    &fm,
                    Syntax::Es(EsSyntax {
                        jsx: true,
                        import_attributes: true,
                        ..Default::default()
                    }),
                    EsVersion::Es2022,
                    None,
                    &mut errors,
                ).unwrap();

                Ok(ModuleData {
                    fm,
                    module,
                    helpers: Default::default(),
                })
            }
        }

        // Implement the Resolve trait to resolve module paths
        struct SimpleResolver;

        impl Resolve for SimpleResolver {
            fn resolve(&self, base: &FileName, dep: &str) -> Result<swc_ecma_loader::resolve::Resolution, anyhow::Error> {
                let base_path = match base {
                    FileName::Real(path) => path.parent().unwrap(),
                    _ => Path::new("."),
                };

                let dep_path = base_path.join(dep);

                let dep_path = dep_path.canonicalize()?;
                Ok(Resolution { filename: FileName::Real(dep_path), slug: None })
            }
        }

        // Dummy Hook for Bundler
        struct HookDummy;

        impl Hook for HookDummy {}

        // Set the global compiler context
        swc_common::GLOBALS.set(&globals, || {
            let fs_loader = FsLoader {
                cm,
                handler,
            };

            let resolver = SimpleResolver;

            let mut bundler = Bundler::new(
                &globals,
                cm.clone(),
                fs_loader,
                resolver,
                swc_bundler::Config {
                    require: false,
                    disable_inliner: false,
                    external_modules: Vec::new(),
                    ..Default::default()
                },
                Box::new(HookDummy),
            );

            // Entry file
            let entries = vec![(String::from("main"), FileName::Real(js_path.to_path_buf()))];

            let modules = bundler.bundle(entries).map_err(|e| CommonError::Error(format!("Failed to bundle: {:?}", e)))?;

            for bundled_module in modules {
                // Output the bundled code to the output folder
                let code = {
                    let mut buf = vec![];
                    let mut emitter = Emitter {
                        cfg: CodegenConfig {
                            minify: self.base.config.minify, // Assuming minify field in config
                            ..Default::default()
                        },
                        cm: cm.clone(),
                        comments: None,
                        wr: Box::new(JsWriter::new(
                            cm.clone(),
                            "\n",
                            &mut buf,
                            None,
                        )),
                    };

                    bundled_module.module.emit_with(&mut emitter).map_err(|e| CommonError::Error(format!("Failed to emit code: {:?}", e)))?;

                    String::from_utf8(buf).map_err(|e| CommonError::Error(format!("Failed to convert output to UTF-8: {:?}", e)))?
                };

                // Write the code to the output file
                let output_file_path = output_folder.join("bundle.js"); // Adjust output filename as needed
                std::fs::write(&output_file_path, code).map_err(|e| CommonError::Error(format!("Failed to write output file: {:?}", e)))?;
            }

            println!(
                "Compiled JavaScript file {:?} to {:?}",
                js_path,
                output_folder
            );
            Ok(())
        })
    }
}