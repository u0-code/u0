// @ts-check
// The guide's order. Each label is translated in
// i18n/<locale>/docusaurus-plugin-content-docs/current.json.

/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  guide: [
    'intro',
    {
      type: 'category',
      label: 'Getting started',
      collapsed: false,
      items: ['install', 'first-run'],
    },
    {
      type: 'category',
      label: 'Working with agents',
      collapsed: false,
      items: [
        'projects-and-sessions',
        'thread',
        'worktrees-and-checkpoints',
        'review',
        'forges',
        'simulators',
      ],
    },
    {
      type: 'category',
      label: 'Automation',
      collapsed: false,
      items: ['automation', 'tools-for-agents'],
    },
    'remote',
    {
      type: 'category',
      label: 'Reference',
      items: [
        'reference/settings',
        'reference/keybindings',
        'reference/cli',
        'reference/policy',
      ],
    },
    'troubleshooting',
    'privacy-security',
  ],
};

export default sidebars;
