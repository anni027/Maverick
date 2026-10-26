// Maverick frontend entry point
import React from 'react';
import ReactDOM from 'react-dom/client';
// Self-hosted: the Google Fonts @import fell back to Segoe UI whenever offline.
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/inter/700.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/500.css';
import App from './App';
import { ThemeProvider } from './hooks/useTheme';
import './index.css';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </React.StrictMode>
);