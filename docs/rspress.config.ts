import * as path from 'node:path';
import { defineConfig } from '@rspress/core';

export default defineConfig({
  root: path.join(__dirname, 'docs'),
  title: 'pgorm',
  description: 'A PostgreSQL ORM library for Rust',
  icon: '/rspress-icon.png',
  logo: {
    light: '/rspress-light-logo.png',
    dark: '/rspress-dark-logo.png',
  },
  locales: [
    {
      lang: 'en',
      label: 'English',
    },
    {
      lang: 'zh',
      label: '简体中文',
    },
  ],
  themeConfig: {
    socialLinks: [
      {
        icon: 'github',
        mode: 'link',
        content: 'https://github.com/fankaiLiu/pgorm',
      },
    ],
    locales: [
      {
        lang: 'en',
        label: 'English',
        outlineTitle: 'On This Page',
        prevPageText: 'Previous',
        nextPageText: 'Next',
        searchPlaceholderText: 'Search',
      },
      {
        lang: 'zh',
        label: '简体中文',
        outlineTitle: '本页目录',
        prevPageText: '上一页',
        nextPageText: '下一页',
        searchPlaceholderText: '搜索',
      },
    ],
  },
});
