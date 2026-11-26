// @ts-check
// Ghost's user guide: docs only, served at the site's root, in English and
// Simplified Chinese. Built and published to GitHub Pages by
// .github/workflows/docs.yml.

import {themes as prismThemes} from 'prism-react-renderer';

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'Ghost',
  tagline: 'One desktop app for the coding agents you already use',
  favicon: 'img/logo.svg',

  future: {
    v4: true,
  },

  url: 'https://u0-code.github.io',
  baseUrl: '/u0/',
  organizationName: 'u0-code',
  projectName: 'u0',
  trailingSlash: false,

  onBrokenLinks: 'throw',
  onBrokenAnchors: 'throw',
  // Pages are plain Markdown (.md is CommonMark, not MDX), so a `{{body}}`
  // or a `<home>` in the text is text.
  markdown: {
    format: 'detect',
    hooks: {onBrokenMarkdownLinks: 'throw'},
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en', 'zh-Hans'],
    localeConfigs: {
      en: {label: 'English'},
      'zh-Hans': {label: '简体中文'},
    },
  },

  presets: [
    [
      'classic',
      /** @type {import('@docusaurus/preset-classic').Options} */
      ({
        docs: {
          routeBasePath: '/',
          sidebarPath: './sidebars.js',
          editUrl: 'https://github.com/u0-code/u0/tree/main/website/',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      }),
    ],
  ],

  // Search built into the pages at build time, in both languages: no
  // service, no key, nothing a reader types sent anywhere.
  themes: [
    [
      '@easyops-cn/docusaurus-search-local',
      ({
        hashed: true,
        docsRouteBasePath: '/',
        indexBlog: false,
        language: ['en', 'zh'],
      }),
    ],
  ],

  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    ({
      colorMode: {
        respectPrefersColorScheme: true,
      },
      navbar: {
        title: 'Ghost',
        logo: {
          alt: 'Ghost',
          src: 'img/logo.svg',
        },
        items: [
          {type: 'localeDropdown', position: 'right'},
          {
            href: 'https://github.com/u0-code/u0',
            label: 'GitHub',
            position: 'right',
          },
        ],
      },
      footer: {
        style: 'dark',
        copyright: 'Ghost · Apache-2.0',
      },
      prism: {
        theme: prismThemes.github,
        darkTheme: prismThemes.dracula,
        additionalLanguages: ['bash', 'json'],
      },
    }),
};

export default config;
