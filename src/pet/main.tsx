import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createRoot } from 'react-dom/client';
import '../ds';
import { I18nProvider, langSetting } from '../shared/i18n';
import { applyPlatformAttribute } from '../shared/platform';
import { useSetting } from '../shared/queries';
import { applyTheme } from '../shared/theme';
import { PetApp } from './PetApp';

applyPlatformAttribute();
applyTheme();

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
});

function Root() {
  const language = useSetting<string>('ui.language', 'system');
  return (
    <I18nProvider setting={langSetting(language)}>
      <PetApp />
    </I18nProvider>
  );
}

createRoot(document.getElementById('root')!).render(
  <QueryClientProvider client={queryClient}>
    <Root />
  </QueryClientProvider>,
);
