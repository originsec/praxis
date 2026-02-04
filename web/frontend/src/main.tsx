import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './index.css';
import App from './App';
import { initFeatureFlags } from './utils/featureFlags';
import { ThemeProvider } from './context/ThemeContext';

//
// Initialize devtools feature flags (window.praxis).
//
initFeatureFlags();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </StrictMode>
);
