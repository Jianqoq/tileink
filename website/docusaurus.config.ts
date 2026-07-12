import type {Config} from '@docusaurus/types';
import type {Options, ThemeConfig} from '@docusaurus/preset-classic';
import {themes as prismThemes} from 'prism-react-renderer';

const config: Config = {
  title: 'Tileink',
  tagline: 'Tile-based vector rendering for Rust and WGPU',
  favicon: 'img/favicon.svg',
  url: 'http://localhost',
  baseUrl: '/',
  onBrokenLinks: 'throw',
  markdown: {
    mermaid: true,
    hooks: {onBrokenMarkdownLinks: 'throw'},
  },
  themes: ['@docusaurus/theme-mermaid'],
  i18n: {
    defaultLocale: 'zh-Hans',
    locales: ['zh-Hans', 'en'],
    localeConfigs: {
      'zh-Hans': {label: '简体中文', htmlLang: 'zh-Hans'},
      en: {label: 'English', htmlLang: 'en'},
    },
  },
  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          routeBasePath: 'docs',
          showLastUpdateTime: true,
        },
        blog: false,
        theme: {customCss: './src/css/custom.css'},
      } satisfies Options,
    ],
  ],
  themeConfig: {
    colorMode: {defaultMode: 'dark', respectPrefersColorScheme: true},
    navbar: {
      title: 'Tileink',
      logo: {alt: 'Tileink', src: 'img/logo.svg'},
      items: [
        {type: 'docSidebar', sidebarId: 'tutorialSidebar', position: 'left', label: '文档'},
        {to: '/docs/architecture/overview', label: '架构', position: 'left'},
        {to: '/docs/api/overview', label: 'API', position: 'left'},
        {type: 'localeDropdown', position: 'right'},
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {title: '学习', items: [{label: '快速开始', to: '/docs/getting-started/quick-start'}, {label: '架构', to: '/docs/architecture/overview'}]},
        {title: '参考', items: [{label: 'Canvas API', to: '/docs/api/canvas'}, {label: 'Retained API', to: '/docs/api/retained-scene'}]},
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Tileink contributors.`,
    },
    prism: {theme: prismThemes.github, darkTheme: prismThemes.dracula, additionalLanguages: ['rust', 'toml', 'powershell']},
    mermaid: {theme: {light: 'neutral', dark: 'dark'}},
  } satisfies ThemeConfig,
};

export default config;
