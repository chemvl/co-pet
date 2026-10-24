import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '../ds';
import { I18nProvider, langSetting } from '../shared/i18n';
import { applyPlatformAttribute } from '../shared/platform';
import { useSetting } from '../shared/queries';
import { applyTheme } from '../shared/theme';
import { ToastProvider } from '../shared/Toast';
import { HomeApp } from './HomeApp';
import './home.css';

applyPlatformAttribute();
applyTheme();

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
});

function Root() {
  const language = useSetting<string>('ui.language', 'system');
  return (
    <I18nProvider setting={langSetting(language)}>
      <ToastProvider>
        <HomeApp />
      </ToastProvider>
    </I18nProvider>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <Root />
    </QueryClientProvider>
  </StrictMode>,
);
