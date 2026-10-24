import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createRoot } from 'react-dom/client';
import '../ds';
import { I18nProvider, langSetting } from '../shared/i18n';
import { applyPlatformAttribute } from '../shared/platform';
import { useSettings } from '../shared/queries';
import { applyTheme } from '../shared/theme';
import { ToastProvider } from '../shared/Toast';
import { OnboardingApp } from './OnboardingApp';

applyPlatformAttribute();
applyTheme();

const queryClient = new QueryClient();

function Root() {
  const { data: settings } = useSettings();
  // Wait for settings so the first paint is already in the chosen language.
  if (!settings) return null;
  return (
    <I18nProvider setting={langSetting(settings['ui.language'])}>
      <ToastProvider>
        <OnboardingApp />
      </ToastProvider>
    </I18nProvider>
  );
}

createRoot(document.getElementById('root')!).render(
  <QueryClientProvider client={queryClient}>
    <Root />
  </QueryClientProvider>,
);
