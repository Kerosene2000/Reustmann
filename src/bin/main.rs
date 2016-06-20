#[macro_use] extern crate colorify;
#[macro_use] extern crate nom;
extern crate rustyline;
extern crate reustmann;

mod command;
mod debugger;

use std::io::{empty, sink};
use std::error::Error;
use std::fs::File;
use rustyline::completion::FilenameCompleter;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use reustmann::{DebugInfos, Statement}; // FIXME move this elsewhere
use reustmann::instruction::{ Instruction, LongMnemonic, Mnemonic, OpCode, is_valid_op_code};
use command::Command;
use debugger::Debugger;
use reustmann::Program;

fn create_program_from_file(filename: &String, ignore_nl: bool) -> Result<Program, String> {
    let mut file = match File::open(filename) {
        Err(err) => return Err(err.description().into()),
        Ok(file) => file,
    };
    let program = match Program::new(&mut file, ignore_nl) {
        Err(err) => return Err(err.into()),
        Ok(program) => program,
    };
    Ok(program)
}

// FIXME move this elsewhere
fn display_statement(statement: Option<Statement>) {
    print!("Last instruction was ");
    match statement {
        Some(statement) => {
            let Statement(op_code, is_success) = statement;
            let name: LongMnemonic = Into::<Instruction>::into(op_code).into();
            println!("'{}' and return '{}'.", name, is_success)
        },
        None => println!("not in this dimension."),
    }
}

fn format_program_counter(mem_addr: usize, offset: usize, op_code: OpCode) -> String {
    let instr: Instruction = op_code.into();
    let longmnemo: LongMnemonic = instr.into();
    let mem_addr = format!(colorify!(blue: "{:>#06x}"), mem_addr);
    let longmnemo = format!(colorify!(green: "{:<6}"), longmnemo);

    let op_code = match is_valid_op_code(op_code) {
        true => format!("{:#04x}, '{}'", op_code, Into::<Mnemonic>::into(instr)),
        false => format!("{:#04x}, '{}'", op_code, op_code as char),
    };
    format!("{} <{:+}>: {} ({})", mem_addr, offset, longmnemo, op_code)
}

fn format_stack_pointer(mem_addr: usize, value: u8) -> String {
    let mem_addr = format!(colorify!(blue: "{:>#06x}"), mem_addr);
    let preview = value as char;
    if preview.is_alphabetic() == true {
        format!("{} ({:#04x}, '{}')", mem_addr, value, preview)
    }
    else {
        format!("{} ({:#04x})", mem_addr, value)
    }
}

// FIXME move this elsewhere
fn display_infos(debug_infos: &DebugInfos, statement: Option<Statement>) {
    let &DebugInfos{ ref memory, pc, sp, nz } = debug_infos;
    println!("pc: {}, sp: {}, nz: {}", pc, sp, nz);
    display_statement(statement);

    // FIXME don't zip, display different number of stack/instructions
    let lines = 10;

    let instrs = (*memory).iter().enumerate().cycle().skip(pc).take(lines).enumerate();
    let stack = (*memory).iter().enumerate().rev().cycle().skip((*memory).len() - sp - 1).take(lines); // FIXME ugly ?
    let mut pc_sp = instrs.zip(stack);

    if let Some(((idx, (pc_addr, op_code)), (sp_addr, value))) = pc_sp.next() {
        let pc_side = format_program_counter(pc_addr, idx, *op_code);
        let pc_side = format!("{} {}", colorify!(red: "pc"), pc_side);
        let sp_side = format_stack_pointer(sp_addr, *value);
        let sp_side = format!("{} {}", colorify!(red: "sp"), sp_side);
        println!("{}    {}", pc_side, sp_side);
    }

    for ((idx, (pc_addr, op_code)), (sp_addr, value)) in pc_sp {
        let pc_side = format_program_counter(pc_addr, idx, *op_code);
        let pc_side = format!("   {}", pc_side);
        let sp_side = format_stack_pointer(sp_addr, *value);
        let sp_side = format!("   {}", sp_side);
        println!("{}    {}", pc_side, sp_side);
    }
}

fn main() {
    let file_comp = FilenameCompleter::new();
    let mut rustyline = Editor::new();

    rustyline.set_completer(Some(&file_comp));
    if let Err(_) = rustyline.load_history("history.txt") {
        printlnc!(yellow: "No previous history.");
    }

    let mut last_command = None;

    let arch_width = 8; // TODO get input source length by default
    let arch_length = 50;
    // FIXME don't unwrap
    let mut dbg = match Debugger::new(arch_length, arch_width) {
        Err(err) => {
            printlnc!(red: "{}", err);
            std::process::exit(1)
        },
        Ok(dbg) => dbg,
    };

    // TODO make it clearer/beautiful
    printlnc!(yellow: "Interpreter informations:");
    printlnc!(yellow: "Arch width:  {}", arch_width);
    printlnc!(yellow: "Arch length: {}", arch_length);

    // Interpreter as an arch width of 8 and an arch length of 6.

    let mut input = empty();
    // let mut output = sink();
    let mut output = std::io::stdout();

    let mut statement = None;

    loop {
        let prompt = format!(colorify!(dark_grey: "({}) "), "rmdb");
        let readline = rustyline.readline(&prompt);
        match readline {
            Ok(line) => {
                rustyline.add_history_entry(&line);

                let command = match (line.parse(), last_command) {
                    (Ok(Command::Repeat), Some(c)) => Ok(c),
                    (Ok(Command::Repeat), None) => Err("No last command.".into()),
                    (Ok(c), _) => Ok(c),
                    (Err(e), _) => Err(e),
                };

                match command {
                    Ok(Command::Infos) => display_infos(&dbg.debug_infos(), statement),
                    Ok(Command::Copy(ref filename, ignore_nl)) => {
                        match create_program_from_file(&filename, ignore_nl) {
                            Err(err) => printlnc!(red: "{}", err),
                            Ok(program) => {
                                match dbg.copy_program_and_reset(&program) {
                                    Err(err) => printlnc!(red: "{}", err),
                                    Ok(_) => {
                                        printlnc!(yellow: "Program correctly loaded.");
                                        display_infos(&dbg.debug_infos(), statement)
                                    },
                                }
                            },
                        }
                    },
                    Ok(Command::Reset) => {
                        statement = Some(dbg.reset());
                        printlnc!(yellow: "Reset.");
                        display_infos(&dbg.debug_infos(), statement)
                    },
                    Ok(Command::Step(to_execute)) => {
                        let (executed, debug, stat) = dbg.steps(to_execute, &mut input, &mut output);
                        statement = stat;
                        match executed == to_execute {
                            true => printlnc!(yellow: "{} steps executed.", executed),
                            false => printlnc!(yellow: "{} steps executed on {}.", executed, to_execute),
                        }
                        display_infos(&debug, statement)
                    },
                    Ok(Command::Exit) => break,
                    Ok(Command::Repeat) => unreachable!(),
                    Err(ref e) => printlnc!(red: "{}", e),
                    // Err(_) => printlnc!(red: "Unrecognized command '{}'.", command),
                };
                last_command = command.ok();
            },
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break
            },
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break
            },
            Err(err) => {
                println!("Error: {:?}", err);
                break
            }
        }
    }
    rustyline.save_history("history.txt").unwrap();
}
