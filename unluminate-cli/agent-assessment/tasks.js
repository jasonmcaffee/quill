// What the model is asked to do, and how each answer is checked.
//
// Every task is one instruction, phrased the way a person would say it — never using the name of the
// command, because a task that names the command is measuring copying rather than understanding.
// `check` runs a `unluminate-cli` command against the live window afterwards and tests the answer, so a
// task passes only if the window really ended up in the state the instruction asked for.
//
// `setup` puts the window into a known state first. It is run through the CLI directly and is not
// part of what is scored.

const PROJECT_MARK = 'assessment-project';

// The window as a fresh Unluminate on the sample project would be.
//
// The project folder is filled in by the harness, because one task deliberately points the window at
// a different folder and every later task would then be reading the wrong project. Re-rooting first
// is what makes each task independent of the one before it.
function fresh(project) {
  return [['project', 'open', project]].concat(FRESH);
}

const FRESH = [
  ['modal', 'cancel'],
  ['terminal', 'hide'],
  ['explorer', 'show'],
  ['explorer', 'filter'],
  ['explorer', 'width', '248'],
  ['settings', 'set', 'appearance.font.size', '16'],
  ['settings', 'set', 'editor.line_numbers', 'true'],
  ['settings', 'set', 'appearance.background.opacity', '0.83'],
  ['tab', 'open', 'todo.txt'],
  // Any tab a previous task wiped or typed into comes back from disk, so the next task starts on
  // the file the sample project holds rather than on whatever was left in it.
  ['tab', 'reload', '--discard'],
  ['editor', 'view', 'raw'],
];

const tasks = [
  // ------------------------------------------------------------------------------- files and tabs
  {
    id: 'open-a-file',
    say: 'Open the README so I can read it.',
    check: { run: ['editor', 'status'], test: (r) => /README\.md$/.test(r.path || '') },
  },
  {
    id: 'open-a-nested-file',
    say: 'I want to look at the main program. It is at src/main.rs.',
    check: { run: ['editor', 'status'], test: (r) => /main\.rs$/.test(r.path || '') },
  },
  {
    id: 'open-two-and-keep-both',
    say: 'Open notes.md and then todo.txt, and make sure both stay open in their own tabs.',
    check: {
      run: ['tab', 'list'],
      test: (r) => {
        const names = r.tabs.map((t) => t.name);
        return names.includes('notes.md') && names.includes('todo.txt');
      },
    },
  },
  {
    id: 'list-the-tabs',
    say: 'Which files do I have open?',
    setup: [['tab', 'open', 'notes.md', '--permanent'], ['tab', 'open', 'README.md', '--permanent']],
    expectCommand: 'tab.list',
  },
  {
    id: 'switch-tab-by-name',
    say: 'Bring the notes.md tab to the front.',
    setup: [['tab', 'open', 'notes.md', '--permanent'], ['tab', 'open', 'README.md', '--permanent']],
    check: { run: ['editor', 'status'], test: (r) => r.name === 'notes.md' },
  },
  {
    id: 'close-the-tab',
    say: 'Close the file I am looking at.',
    setup: [['tab', 'open', 'notes.md', '--permanent'], ['tab', 'open', 'README.md', '--permanent']],
    check: {
      run: ['tab', 'list'],
      test: (r) => !r.tabs.some((t) => t.name === 'README.md'),
    },
  },
  {
    id: 'save-the-file',
    say: 'Save what I have been editing.',
    setup: [['tab', 'open', 'notes.md', '--permanent'], ['editor', 'insert', 'x']],
    check: { run: ['editor', 'status'], test: (r) => r.modified === false },
  },
  {
    id: 'reload-from-disk',
    say: 'Throw away my unsaved changes and read the file from disk again.',
    setup: [['tab', 'open', 'notes.md', '--permanent'], ['editor', 'insert', 'scribble']],
    check: { run: ['editor', 'status'], test: (r) => r.modified === false },
  },

  // ------------------------------------------------------------------------------------ the editor
  {
    id: 'read-the-text',
    say: 'Show me the text of the file that is open.',
    setup: [['tab', 'open', 'notes.md', '--permanent']],
    expectCommand: 'editor.text',
  },
  {
    id: 'read-some-lines',
    say: 'Show me just the first three lines of the open file.',
    setup: [['tab', 'open', 'README.md', '--permanent']],
    expectCommand: 'editor.text',
    expectArguments: (a) => Number(a['to-line']) === 3,
  },
  {
    id: 'move-the-caret',
    say: 'Put the cursor on line 4, column 3.',
    setup: [['tab', 'open', 'src/main.rs', '--permanent']],
    check: { run: ['editor', 'caret'], test: (r) => r.line === 4 && r.column === 3 },
  },
  {
    id: 'select-a-range',
    say: 'Select lines 2 to 4 of this file.',
    setup: [['tab', 'open', 'notes.md', '--permanent']],
    check: { run: ['editor', 'status'], test: (r) => r.selection && r.selection.empty === false },
  },
  {
    id: 'select-everything',
    say: 'Select the whole document.',
    setup: [['tab', 'open', 'notes.md', '--permanent']],
    check: {
      run: ['editor', 'status'],
      test: (r) => r.selection.end - r.selection.start >= r.characters - 1,
    },
  },
  {
    id: 'type-something',
    say: 'Type the word BANANA where the cursor is.',
    setup: [['tab', 'open', 'todo.txt', '--permanent'], ['editor', 'caret', '--line', '1', '--column', '1']],
    check: { run: ['editor', 'text'], test: (r) => (r.text || '').includes('BANANA') },
  },
  {
    id: 'undo-it',
    say: 'Undo that.',
    setup: [
      ['tab', 'open', 'todo.txt', '--permanent'],
      ['editor', 'caret', '--line', '1', '--column', '1'],
      ['editor', 'insert', 'MISTAKE'],
    ],
    check: { run: ['editor', 'text'], test: (r) => !(r.text || '').includes('MISTAKE') },
  },
  {
    id: 'redo-it',
    say: 'I undid that by mistake. Put it back.',
    setup: [
      ['tab', 'open', 'todo.txt', '--permanent'],
      ['editor', 'caret', '--line', '1', '--column', '1'],
      ['editor', 'insert', 'MISTAKE'],
      ['editor', 'undo'],
    ],
    check: { run: ['editor', 'text'], test: (r) => (r.text || '').includes('MISTAKE') },
  },
  {
    id: 'show-the-preview',
    say: 'Show me the rendered version of this Markdown rather than the source.',
    setup: [['tab', 'open', 'README.md', '--permanent']],
    check: { run: ['editor', 'status'], test: (r) => r.viewMode === 'preview' },
  },
  {
    id: 'show-side-by-side',
    say: 'Put the Markdown source and its rendering next to each other.',
    setup: [['tab', 'open', 'README.md', '--permanent']],
    check: { run: ['editor', 'status'], test: (r) => r.viewMode === 'side' },
  },
  {
    id: 'back-to-the-source',
    say: 'Go back to showing me the raw Markdown.',
    setup: [['tab', 'open', 'README.md', '--permanent'], ['editor', 'view', 'preview']],
    check: { run: ['editor', 'status'], test: (r) => r.viewMode === 'raw' },
  },

  // ---------------------------------------------------------------------------------- the explorer
  {
    id: 'hide-the-explorer',
    say: 'Collapse the file tree on the left, I want more room.',
    check: { run: ['status'], test: (r) => r.explorer.visible === false },
  },
  {
    id: 'show-the-explorer',
    say: 'Bring the file tree back.',
    setup: [['explorer', 'hide']],
    check: { run: ['status'], test: (r) => r.explorer.visible === true },
  },
  {
    id: 'widen-the-explorer',
    say: 'Make the file tree 400 points wide.',
    check: { run: ['status'], test: (r) => Math.round(r.explorer.width) === 400 },
  },
  {
    id: 'filter-the-tree',
    say: 'Filter the file tree down to things with "notes" in the name.',
    check: { run: ['status'], test: (r) => (r.explorer.filter || '').includes('notes') },
  },
  {
    id: 'open-a-folder-in-the-tree',
    say: 'Open up the src folder in the tree.',
    expectCommand: 'explorer.expand',
    expectArguments: (a) => /src/.test(a.path || ''),
  },
  {
    id: 'list-the-project-files',
    say: 'What files are in this project?',
    expectCommand: ['explorer.files', 'explorer.tree'],
  },

  // ------------------------------------------------------------------------------------ the modals
  {
    id: 'go-to-file',
    say: 'I cannot remember where it is — find me the file called markdown.rs and open it.',
    check: { run: ['editor', 'status'], test: (r) => /markdown\.rs$/.test(r.path || '') },
    allowMany: true,
  },
  {
    id: 'go-to-file-open-box',
    say: 'Open the box that finds a file by typing part of its name, and type "guide" into it.',
    check: {
      run: ['modal', 'state'],
      test: (r) => r.open === 'go-to-file' && (r.query || '').includes('guide'),
    },
  },
  {
    id: 'find-in-files',
    say: 'Search every file in the project for the word PORCUPINE.',
    check: {
      run: ['modal', 'state'],
      test: (r) => r.open === 'find-in-files' && (r.query || '').includes('PORCUPINE'),
    },
  },
  {
    id: 'read-the-search-results',
    say: 'What did that search find? Wait for it to finish.',
    setup: [['modal', 'open', 'find-in-files', '--query', 'PORCUPINE']],
    expectCommand: 'modal.results',
  },
  {
    id: 'shut-the-modal',
    say: 'Close that dialog without doing anything.',
    setup: [['modal', 'open', 'go-to-file', '--query', 'notes']],
    check: { run: ['modal', 'state'], test: (r) => !r.open },
  },
  {
    id: 'open-the-settings',
    say: 'Open the settings window.',
    check: { run: ['modal', 'state'], test: (r) => r.open === 'settings' },
  },
  {
    id: 'settings-on-a-page',
    say: 'Open the settings and take me to the terminal page.',
    check: {
      run: ['modal', 'state'],
      test: (r) => r.open === 'settings' && /terminal/i.test(r.page || ''),
    },
  },
  {
    id: 'move-the-modal',
    say: 'That dialog is in the way — drag it to 60 across and 60 down.',
    setup: [['modal', 'open', 'go-to-file', '--query', 'notes']],
    expectCommand: 'modal.move',
  },

  // ---------------------------------------------------------------------------------- the settings
  {
    id: 'bigger-font',
    say: 'The text is too small. Make the editor font 24 point.',
    check: { run: ['settings', 'get', 'appearance.font.size'], test: (r) => Number(r.value) === 24 },
  },
  {
    id: 'more-transparent',
    say: 'Let more of my desktop show through the window — set it to 40 percent opaque.',
    check: {
      run: ['settings', 'get', 'appearance.background.opacity'],
      test: (r) => Math.abs(Number(r.value) - 0.4) < 0.02,
    },
  },
  {
    id: 'line-numbers-off',
    say: 'Turn off the line numbers down the side of the editor.',
    check: { run: ['settings', 'get', 'editor.line_numbers'], test: (r) => r.value === 'false' },
  },
  {
    id: 'terminal-font',
    say: 'Make the terminal text 18 point.',
    check: { run: ['settings', 'get', 'terminal.font.size'], test: (r) => Number(r.value) === 18 },
  },
  {
    id: 'what-can-i-change',
    say: 'What settings can I change?',
    expectCommand: 'settings.list',
  },

  // ---------------------------------------------------------------------------------- the terminal
  {
    id: 'open-a-terminal',
    say: 'Open a terminal at the bottom.',
    check: { run: ['status'], test: (r) => r.terminal.visible === true },
  },
  {
    id: 'run-a-command',
    say: 'In the terminal, run "echo hello-from-the-agent".',
    setup: [['terminal', 'show']],
    check: {
      run: ['terminal', 'read', '--wait-for', 'hello-from-the-agent', '--timeout', '15000'],
      test: (r) => (r.text || '').includes('hello-from-the-agent'),
    },
    allowMany: true,
  },
  {
    id: 'second-terminal',
    say: 'Give me a second terminal tab.',
    setup: [['terminal', 'show']],
    check: { run: ['terminal', 'list'], test: (r) => r.count >= 2 },
  },
  {
    id: 'read-the-terminal',
    say: 'What is on the terminal screen at the moment?',
    setup: [['terminal', 'show']],
    expectCommand: 'terminal.read',
  },
  {
    id: 'put-the-terminal-away',
    say: 'Hide the terminal again.',
    setup: [['terminal', 'show']],
    check: { run: ['status'], test: (r) => r.terminal.visible === false },
  },

  // ------------------------------------------------------------------------- git, actions, pictures
  {
    id: 'git-state',
    say: 'What does git think of this project?',
    expectCommand: 'git.status',
  },
  {
    id: 'take-a-picture',
    say: 'Take a screenshot of the window and put it in shot.png.',
    expectCommand: 'window.screenshot',
    expectArguments: (a) => /shot\.png$/.test(a.file || ''),
  },
  {
    id: 'where-am-i',
    say: 'Tell me everything about the window right now.',
    expectCommand: 'status',
  },
  {
    id: 'run-a-menu-entry',
    say: 'Use the menu entry that toggles the line numbers, rather than the setting.',
    expectCommand: 'action.run',
    expectArguments: (a) => (a.name || '') === 'toggle-line-numbers',
  },
  {
    id: 'what-is-running',
    say: 'Are there any Unluminate windows running, and on what?',
    expectCommand: 'instances',
  },
  {
    id: 'the-about-box',
    say: 'What version of the Unluminate editor am I running?',
    expectCommand: ['action.run', 'status'],
  },

  // ------------------------------------------------------------------- the harder half (round three)
  {
    id: 'save-a-copy',
    say: 'Save what I am looking at as a second file called copy.md, and carry on in that one.',
    setup: [['tab', 'open', 'notes.md', '--permanent']],
    check: { run: ['editor', 'status'], test: (r) => /copy\.md$/.test(r.path || '') },
  },
  {
    id: 'replace-the-whole-file',
    say: 'Wipe out everything in this file and put the single line HELLO in it instead.',
    setup: [['tab', 'open', 'todo.txt', '--permanent']],
    check: {
      run: ['editor', 'text'],
      test: (r) => (r.text || '').trim() === 'HELLO',
    },
  },
  {
    id: 'find-then-open',
    say: 'Find the file whose name matches "mdrs" and open the first thing it finds.',
    check: { run: ['editor', 'status'], test: (r) => /markdown\.rs$/.test(r.path || '') },
    allowMany: true,
  },
  {
    id: 'search-and-jump',
    say: 'Find PORCUPINE somewhere in the project and take me to it.',
    check: {
      run: ['editor', 'status'],
      test: (r) => /guide\.md$/.test(r.path || ''),
    },
    allowMany: true,
  },
  {
    id: 'resize-the-modal',
    say: 'That dialog is too small — make it 900 wide and 600 tall.',
    setup: [['modal', 'open', 'find-in-files', '--query', 'note']],
    expectCommand: 'modal.size',
    expectArguments: (a) => Number(a.width) === 900 && Number(a.height) === 600,
  },
  {
    id: 'resize-the-window',
    say: 'Make the Unluminate window exactly 1000 by 700.',
    expectCommand: 'window.size',
    expectArguments: (a) => Number(a.width) === 1000 && Number(a.height) === 700,
  },
  {
    id: 'interrupt-the-shell',
    say: 'Something is stuck in the terminal — send it a Control-C.',
    setup: [['terminal', 'show']],
    expectCommand: 'terminal.send',
    expectArguments: (a) => /ctrl-c/i.test(a.key || ''),
  },
  {
    id: 'type-without-running',
    say: 'Put "cargo build" on the terminal prompt but do not run it yet.',
    setup: [['terminal', 'show']],
    expectCommand: 'terminal.send',
    expectArguments: (a) => a['no-enter'] === true && /cargo build/.test(a.text || ''),
  },
  {
    id: 'what-can-git-do',
    say: 'What things can I ask git to do from here?',
    expectCommand: 'git.actions',
  },
  {
    id: 'which-languages',
    say: 'Which languages does this editor know how to colour?',
    expectCommand: 'plugins.list',
  },
  {
    id: 'projects-i-had-open',
    say: 'Which projects have I had open before?',
    expectCommand: 'project.recent',
  },
  {
    id: 'put-a-setting-back',
    say: 'Put the editor font size back to whatever a brand new Unluminate uses.',
    setup: [['settings', 'set', 'appearance.font.size', '32']],
    check: { run: ['settings', 'get', 'appearance.font.size'], test: (r) => Number(r.value) === 16 },
  },
  {
    id: 'which-fonts',
    say: 'What fonts could I set the editor to?',
    expectCommand: 'settings.fonts',
  },
  {
    id: 'a-note-in-the-status-bar',
    say: 'Put the words "step three done" in the status bar at the bottom.',
    check: { run: ['status'], test: (r) => (r.message || '').includes('step three done') },
  },
  {
    id: 'change-project',
    say: 'Show me the src folder as the project in this window instead.',
    expectCommand: 'project.open',
    expectArguments: (a) => /src/.test(a.folder || ''),
  },
];

module.exports = { tasks, FRESH, fresh, PROJECT_MARK };
