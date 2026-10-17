// Meetings (Meetings.dc.html): the meeting list, the open meeting's transcript and its summary.
// With no meeting picked the first one opens; below 1100 px an open meeting hides the list and
// gets a Back button, and without one the list fills the page.
import { useQuery } from '@tanstack/react-query';
import type { ReactElement } from 'react';
import { Button } from '../../ds';
import { errorMessage } from '../../shared/errors';
import { useT } from '../../shared/i18n';
import { useDelayed, useMediaQuery } from '../hooks';
import { EmptyState, Skeleton } from '../InboxPage';
import { meetingQuery, meetingsQuery } from '../queries';
import { navigate, type Route } from '../router';
import { MeetingList } from './MeetingList';
import { useMeetingActions } from './recordFlow';
import { SummaryPane } from './SummaryPane';
import { Transcript } from './Transcript';
import './meetings.css';

const SKELETON_DELAY_MS = 400;

export function MeetingsPage({ route }: { route: Extract<Route, { page: 'meetings' }> }): ReactElement {
  const t = useT();
  const actions = useMeetingActions();
  const wide = useMediaQuery('(min-width: 1100px)');
  const list = useQuery(meetingsQuery());
  const slow = useDelayed(list.isPending, SKELETON_DELAY_MS);
  const meetings = list.data ?? [];
  const selectedId = route.id ?? (wide ? meetings[0]?.id : undefined);

  let content: ReactElement;
  if (list.isError) {
    content = (
      <div className="hm-listmsg">
        <span className="body">{errorMessage(t, list.error)}</span>
        <Button variant="secondary" size="sm" onClick={() => void list.refetch()}>
          {t('common.retry')}
        </Button>
      </div>
    );
  } else if (list.isPending) {
    content = slow ? <Skeleton label={t('meetings.loading')} /> : <></>;
  } else if (!meetings.length && !route.id) {
    content = (
      <EmptyState
        title={t('meetings.empty.title')}
        line={t('meetings.empty.line')}
        action={{ label: t('meetings.record'), run: actions.record }}
      />
    );
  } else {
    content = (
      <div className="mt-page">
        {wide || !selectedId ? <MeetingList meetings={meetings} selectedId={selectedId} full={!selectedId} /> : null}
        {selectedId ? <MeetingView key={selectedId} id={selectedId} onBack={wide ? undefined : () => navigate('meetings')} /> : null}
      </div>
    );
  }
  return <div className="hm-page">{content}</div>;
}

function MeetingView({ id, onBack }: { id: string; onBack?: () => void }): ReactElement {
  const t = useT();
  const q = useQuery(meetingQuery(id));
  const slow = useDelayed(q.isPending, SKELETON_DELAY_MS);
  if (q.data) {
    return (
      <>
        <Transcript meeting={q.data} onBack={onBack} />
        <SummaryPane meeting={q.data} />
      </>
    );
  }
  if (q.isError) {
    return (
      <div className="mt-tr">
        <div className="mt-msg" role="alert">
          <p className="body">{errorMessage(t, q.error)}</p>
          <Button variant="secondary" size="sm" onClick={onBack ?? (() => navigate('meetings'))}>
            {t('common.back')}
          </Button>
        </div>
      </div>
    );
  }
  return <div className="mt-tr">{slow ? <div className="hm-skeleton__block" aria-busy /> : null}</div>;
}
