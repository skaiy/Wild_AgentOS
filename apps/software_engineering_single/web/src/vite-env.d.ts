/// <reference types="vite/client" />

declare module '*.module.css' {
  const classes: { readonly [key: string]: string };
  export default classes;
}

declare module '@chatscope/chat-ui-kit-styles/dist/default/styles.min.css';
