use std::collections::HashMap;
use crate::contexts::{BuildContext, BaseContext};
use std::path::{Path, PathBuf};
use anyhow::Error;
use serde::Deserialize;
use crate::misc::errors::common::CommonError;
use crate::misc::errors::transfer;

use swc_common::{Globals, SourceMap, FileName, Span};
use swc_bundler::{Bundler, Load, ModuleData, Resolve, Hook, ModuleRecord};
use swc_ecma_parser::{Syntax, parse_file_as_module, EsSyntax};
use swc_ecma_codegen::{Emitter, text_writer::JsWriter, Config as CodegenConfig};
use swc_common::errors::Handler;
use swc_common::sync::Lrc;
use swc_ecma_ast::{Bool, EsVersion, Expr, IdentName, KeyValueProp, Lit, MemberExpr, MemberProp, MetaPropExpr, MetaPropKind, PropName, Str};
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

    fn build(&self, path: Option<&PathBuf>) -> Result<(), CommonError> {
        let output_folder = self.get_output_folder()?;

        // Ensure the output folder exists
        if !output_folder.exists() {
            std::fs::create_dir_all(&output_folder).map_err(transfer::TransferError::IoError)?;
        }
        let mut build_entrypoints = false;
        
        if let Some(path) = path {
            let is_entrypoint = self.get_entrypoints().iter().any(|entry| {
                Path::new(&entry.entrypoint).canonicalize().ok() == path.canonicalize().ok()
            });
            if !is_entrypoint {
                self.build(path.as_path(), &output_folder)?;
            }
            build_entrypoints = is_entrypoint
        } 
        if build_entrypoints {
            for entrypoint in self.get_entrypoints() {
                let entrypoint_path = Path::new(&entrypoint.entrypoint);
                self.build(entrypoint_path, &output_folder)?;
            }
        }
        Ok(())
    }
}

impl Context {
    fn build(&self, js_path: &Path, output_folder: &Path) -> Result<(), CommonError> {
        if !js_path.exists() {
            return Err(CommonError::Error(format!("JavaScript file {:?} does not exist.", js_path)));
        }
        if js_path.is_dir() {
            return Err(CommonError::Error(format!("JavaScript path is a folder {:?}.", js_path)));
        }

        let globals = Box::leak(Box::default());
        let cm: Lrc<SourceMap> = Default::default();
        let handler = Handler::with_tty_emitter(
            swc_common::errors::ColorConfig::Auto,
            true,
            false,
            Some(cm.clone()),
        );

        let mut bundler = Bundler::new(
            globals,
            cm.clone(),
            FsLoader { cm: cm.clone(), handler },
            SimpleResolver,
            swc_bundler::Config {
                require: false,
                disable_inliner: false,
                external_modules: Vec::new(),
                ..Default::default()
            },
            Box::new(DummyHook),
        );

        let entries = HashMap::from([(String::from("main"), FileName::Real(js_path.to_path_buf()))]);
        let modules = bundler
            .bundle(entries)
            .map_err(|e| CommonError::Error(format!("Failed to bundle: {:?}", e)))?;

        // Set the global compiler context
        swc_common::GLOBALS.set(globals, || {
            for bundled_module in modules {
                // Output the bundled code to the output folder
                let code = {
                    let mut buf = vec![];
                    let mut emitter = Emitter {
                        cfg: CodegenConfig::default().with_minify(false), //todo: load value from config
                        cm: cm.clone(),
                        comments: None,
                        wr: Box::new(JsWriter::new(
                            cm.clone(),
                            "\n",
                            &mut buf,
                            None,
                        )),
                    };
                    emitter.emit_module(&bundled_module.module).map_err(|e| CommonError::Error(format!("Failed to emit code: {:?}", e)))?;

                    String::from_utf8(buf).map_err(|e| CommonError::Error(format!("Failed to convert output to UTF-8: {:?}", e)))?
                };

                // Write the code to the output file
                let output_file_path = output_folder.join("bundle.js"); // Adjust output filename as needed
                std::fs::write(&output_file_path, code).map_err(|e| CommonError::Error(format!("Failed to write output file: {:?}", e)))?;
            }

            Ok(())
        })
    }
}

struct DummyHook;

impl Hook for DummyHook {
    fn get_import_meta_props(
        &self,
        span: Span,
        module_record: &ModuleRecord,
    ) -> Result<Vec<KeyValueProp>, Error> {
        let file_name = module_record.file_name.to_string();

        Ok(vec![
            KeyValueProp {
                key: PropName::Ident(IdentName::new("url".into(), span)),
                value: Box::new(Expr::Lit(Lit::Str(Str {
                    span,
                    raw: None,
                    value: file_name.into(),
                }))),
            },
            KeyValueProp {
                key: PropName::Ident(IdentName::new("main".into(), span)),
                value: Box::new(if module_record.is_entry {
                    Expr::Member(MemberExpr {
                        span,
                        obj: Box::new(Expr::MetaProp(MetaPropExpr {
                            span,
                            kind: MetaPropKind::ImportMeta,
                        })),
                        prop: MemberProp::Ident(IdentName::new("main".into(), span)),
                    })
                } else {
                    Expr::Lit(Lit::Bool(Bool { span, value: false }))
                }),
            },
        ])
    }
}

struct FsLoader {
    cm: Lrc<SourceMap>,
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
        );
        if !errors.is_empty() {
            for e in errors {
                e.into_diagnostic(&self.handler).emit();
            }
            return Err(anyhow::Error::msg("Parsing errors occurred."));
        }

        Ok(ModuleData {
            fm,
            module: module.unwrap(),
            helpers: Default::default(),
        })
    }
}

struct SimpleResolver;

impl Resolve for SimpleResolver {
    fn resolve(&self, base: &FileName, dep: &str) -> Result<Resolution, anyhow::Error> {
        let base_path = match base {
            FileName::Real(path) => path.parent().unwrap(),
            _ => Path::new("."),
        };

        let mut dep_path = PathBuf::from(dep);

        if dep_path.is_relative() {
            dep_path = base_path.join(dep_path);
        }

        let dep_path = dep_path.canonicalize()?;
        Ok(Resolution { filename: FileName::Real(dep_path), slug: None })
    }
}