import '@vision-pro/design/styles.css';
import './desktop.css';
import { ToastProvider } from '@vision-pro/design';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App.tsx';

createRoot(document.getElementById('root')!).render(<StrictMode><ToastProvider><App /></ToastProvider></StrictMode>);
