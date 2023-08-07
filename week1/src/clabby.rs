#![allow(dead_code)]

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
};

/// The input to the puzzle
/// Source: https://adventofcode.com/2022/day/7
const PUZZLE_INPUT: &str = r#"
$ cd /
$ ls
dir a
14848514 b.txt
8504156 c.dat
dir d
$ cd a
$ ls
dir e
29116 f
2557 g
62596 h.lst
$ cd e
$ ls
584 i
$ cd ..
$ cd ..
$ cd d
$ ls
4060174 j
8033020 d.log
5626152 d.ext
7214296 k
"#;
/// The [CMD_DELIMITER] is the character that precedes a command in the puzzle input.
const CMD_DELIMITER: char = '$';
/// The [NEWLINE] character, for convenience.
const NEWLINE: char = '\n';
/// The `cd` command, for convenience.
const CD: &str = "cd";
/// The `ls` command, for convenience.
const LS: &str = "ls";
/// The parent directory context, for convenience.
const PARENT_DIR: &str = "..";

/// The [Command] struct represents a command that was ran as well as its output.
#[derive(Debug)]
struct Command {
    /// The command that was executed.
    kind: CommandKind,
    /// The output of the command, including both stderr and stdout, split by newlines.
    /// No distinction is made between stdout and stderr because the puzzle doesn't require it.
    output: Vec<String>,
}

/// Our puzzle input only features 2 commands, `cd` and `ls`. `cd` will have an argument
/// that is the path to the directory to change to, and `ls` will always have no arguments.
#[derive(Debug)]
enum CommandKind {
    Cd(String),
    Ls,
}

impl TryFrom<String> for CommandKind {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        // Commands are structured as: `$ <command> [args]`
        let split = value.split_whitespace().collect::<Vec<&str>>();
        match *split.get(1).ok_or("Failed to parse command")? {
            CD => Ok(Self::Cd(
                split
                    .get(2)
                    .ok_or("Failed to parse command arguments")?
                    .to_string(),
            )),
            LS => Ok(Self::Ls),
            _ => Err("Invalid command"),
        }
    }
}

/// The [FSEntry] struct represents a DAG of files and directories.
#[derive(Debug)]
struct FSEntry {
    pub name: String,
    /// The children of this [FSEntry]. If this is a file, this will be `None`.
    pub children: Option<Vec<SharedFSEntry>>,
    /// The size of the file on disk. If this is a directory, this will be `None`.
    implicit_size: Option<usize>,
}

/// A [SharedFSEntry] is an [FSEntry] that can be shared between multiple owners
/// and mutated within a single-threaded context.
type SharedFSEntry = Rc<RefCell<FSEntry>>;

impl FSEntry {
    fn new(
        name: String,
        children: Option<Vec<SharedFSEntry>>,
        implicit_size: Option<usize>,
    ) -> Self {
        Self {
            name,
            children,
            implicit_size,
        }
    }

    /// Returns the size on disk of the [FSEntry].
    fn size(&self) -> usize {
        if let Some(size) = self.implicit_size {
            size
        } else if let Some(children) = &self.children {
            children.iter().map(|c| c.borrow().size()).sum()
        } else {
            0
        }
    }

    /// Finds the sum of the size of all sub directories that are descendants of Self
    /// that are <= 100_000 bytes in size.
    fn prunable_size(&self) -> usize {
        match self.children {
            Some(ref children) => children
                .iter()
                .map(|c| c.borrow())
                .filter(|c| c.size() <= 100_000 && c.children.is_some())
                .map(|c| c.size() + c.prunable_size())
                .sum(),
            None => 0,
        }
    }
}

/// Reads the puzzle input and returns a vector of [Command]s, which can then
/// be lazily executed to build a directory tree.
fn lex(input: &str) -> Result<VecDeque<Command>, &'static str> {
    // Split the puzzle input into lines.
    let mut lines = input
        .split(NEWLINE)
        .map(|s| s.to_string())
        .collect::<VecDeque<String>>();
    // Allocate initial memory for the vector of commands. We can perform a positive lookahead
    // here to prevent reallocation.
    let mut commands = VecDeque::with_capacity(
        lines
            .iter()
            .filter(|l| l.starts_with(CMD_DELIMITER))
            .count(),
    );

    // Iterate over the lines and build the vector of commands.
    while let Some(command) = lines.pop_front() {
        // Allocate some memory for the command output. This time, we won't perform a positive
        // lookahead to pre-allocate.
        let mut output = Vec::default();

        // Store all output until the next command is found.
        while let Some(line) = lines.front() {
            // Once we've reached the next command, move on.
            if line.starts_with(CMD_DELIMITER) {
                break;
            }
            // Otherwise, store the logline and continue searching.
            output.push(lines.pop_front().ok_or("Failed to pop line from dequeue")?);
        }

        commands.push_back(Command {
            kind: command.try_into()?,
            output,
        });
    }

    Ok(commands)
}

/// To convert a vector of [Command]s into a [FSEntry], we need to build the file tree.
/// A convenient way to do this is by performing a depth-first search to ensure that
/// we can resolve the directory sizes upwards.
///
/// NOTE: This assumes xyz.
fn build_fs(mut cmds: VecDeque<Command>) -> Result<SharedFSEntry, &'static str> {
    // Create the root [FSEntry]. The first command in the list must be `cd`, as defined
    // by the puzzle input.
    let root_context = Rc::new(RefCell::new(match cmds.pop_front() {
        Some(Command {
            kind: CommandKind::Cd(dir_name),
            ..
        }) => Ok(FSEntry::new(dir_name, Some(Vec::default()), None)),
        _ => Err("First command is not `cd`"),
    }?));

    // Keep track of the current and previous contexts.
    let mut current_context = Rc::clone(&root_context);

    // Keep track of directories at each depth.
    let mut depth = 0;
    let mut entries_at_depth = HashMap::<usize, SharedFSEntry>::default();
    entries_at_depth.insert(depth, Rc::clone(&root_context));

    // Iterate over the remaining commands and build the file tree.
    while let Some(command) = cmds.pop_front() {
        match command.kind {
            CommandKind::Cd(dir_name) => match dir_name.as_str() {
                // Move up a directory. We can do this by setting the current context
                // to the parent of the current context.
                PARENT_DIR => match depth {
                    0 => return Err("Attempted to move up from root directory"),
                    _ => {
                        depth -= 1;
                        current_context = Rc::clone(
                            entries_at_depth
                                .get(&depth)
                                .ok_or("Failed to get entry at depth")?,
                        )
                    }
                },
                _ => {
                    // Create a new subdirectory and increment the depth.
                    // Note that this behavior only considers that an unknown directory is a
                    // child of the current context. This is valid in the context of the AoC
                    // puzzle, but not in general.
                    let new_context = Rc::new(RefCell::new(FSEntry::new(
                        dir_name.clone(),
                        Some(Vec::default()),
                        None,
                    )));
                    if let Some(ref mut children) = current_context.borrow_mut().children {
                        children.push(Rc::clone(&new_context));
                        depth += 1;
                        entries_at_depth.insert(depth, Rc::clone(&new_context));
                    }
                    current_context = new_context;
                }
            },
            CommandKind::Ls => {
                if let Some(ref mut children) = current_context.borrow_mut().children {
                    for o in command.output {
                        // Split the output by whitespace to parse the file size and name.
                        let split = o.split_whitespace().collect::<Vec<&str>>();
                        // The file size is the first element in the split.
                        let size = split
                            .first()
                            .ok_or("Failed to parse file size from `ls` output")?
                            .parse::<usize>()
                            .ok();
                        // The file name is the second element in the split.
                        let name = split
                            .get(1)
                            .ok_or("Failed to parse file name from `ls` output")?
                            .to_string();
                        // Allocate a vec for the child if it's a directory.
                        let child_vec = if size.is_some() {
                            None
                        } else {
                            Some(Vec::default())
                        };

                        // Create the child and add it to the current context's children.
                        children.push(Rc::new(RefCell::new(FSEntry::new(name, child_vec, size))));
                    }
                }
            }
        }
    }

    Ok(root_context)
}

#[cfg(test)]
mod test {
    use super::*;

    /// The magic number provided by AoC 7 as the solution to the given puzzle input.
    const MAGIC_NUMBER: usize = 95437;

    #[test]
    fn test_solution() {
        let fs = build_fs(lex(PUZZLE_INPUT.trim()).expect("Lexing should not fail"))
            .expect("Building the file system DAG should not fail");

        assert_eq!(fs.borrow().prunable_size(), MAGIC_NUMBER);
    }
}
