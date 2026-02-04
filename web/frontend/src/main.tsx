import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './index.css';
import './themes/origin_light/index.css';
import App from './App';
import { initFeatureFlags } from './utils/featureFlags';

//
// Initialize devtools feature flags (window.praxis).
//
initFeatureFlags();

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
