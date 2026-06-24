//! Dispatch logic for `zenith workspace`.

use std::process::ExitCode;

use crate::cli::{self, WorkspaceArgs};
use crate::commands;

pub(super) fn dispatch_workspace(args: WorkspaceArgs) -> ExitCode {
    match args.command {
        cli::WorkspaceSub::Scratch(scratch_args) => match scratch_args.command {
            cli::ScratchSub::New(a) => {
                let doc_bytes = match std::fs::read(&a.doc) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("error reading '{}': {}", a.doc.display(), e);
                        return ExitCode::from(2);
                    }
                };
                match commands::workspace::scratch_new(&doc_bytes, &a.doc, &a) {
                    Ok(id) => {
                        println!("{}", id);
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                        ExitCode::from(2)
                    }
                }
            }
            cli::ScratchSub::List(a) => match commands::workspace::scratch_list(&a.doc, a.json) {
                Ok(out) => {
                    println!("{}", out);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e);
                    ExitCode::from(2)
                }
            },
            cli::ScratchSub::Show(a) => {
                match commands::workspace::scratch_show(&a.doc, &a.candidate, a.json) {
                    Ok(out) => {
                        println!("{}", out);
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("{}", e);
                        ExitCode::from(2)
                    }
                }
            }
        },
        cli::WorkspaceSub::Candidate(a) => {
            match commands::workspace::candidate_set_status(&a.doc, &a.candidate, &a.status) {
                Ok(out) => {
                    println!("{}", out);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e);
                    ExitCode::from(2)
                }
            }
        }
        cli::WorkspaceSub::Promote(a) => {
            match commands::workspace::promote(&a.doc, &a.candidate, &a.into, &a.id_suffix) {
                Ok(out) => {
                    println!("{}", out);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}", e);
                    ExitCode::from(2)
                }
            }
        }
        cli::WorkspaceSub::Finalize(a) => match commands::workspace::finalize(&a.doc, a.json) {
            Ok(out) => {
                println!("{}", out);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{}", e);
                ExitCode::from(2)
            }
        },
        cli::WorkspaceSub::Bundle(a) => match commands::workspace::bundle_doc(&a.doc, &a.out) {
            Ok(out) => {
                println!("{}", out);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{}", e);
                ExitCode::from(2)
            }
        },
        cli::WorkspaceSub::Unbundle(a) => match commands::workspace::unbundle_doc(&a.bundle) {
            Ok(doc_id) => {
                println!("{}", doc_id);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{}", e);
                ExitCode::from(2)
            }
        },
    }
}
