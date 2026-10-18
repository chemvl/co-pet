// The five onboarding steps (plan Step 13) and the model download strip.
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useId, type ReactNode } from 'react';
import { Button, Icon, StatusPill } from '../ds';
import { HotkeyRows } from '../home/settings/HotkeyRows';
import {
  aiErrorReason, downloadPercent, modelMeta, modelName, transferOf, useAiAction,
} from '../home/settings/ModelCards';
import { ProgressBar, SpeciesTiles } from '../home/settings/parts';
import { StorageRows } from '../home/settings/StorageRows';
import type { SkinId, Species } from '../pet/catalog';
import { CatArt } from '../pet/CatArt';
import { poseFor, type CatPose } from '../pet/pose';
import { formatSize } from '../shared/format';
import { useLang, useT } from '../shared/i18n';
import { aiDownload, aiPause, aiSelectModel, permissionsStatus, requestPermission } from '../shared/ipc';
import { isWindows } from '../shared/platform';
import { useIntegrations } from '../shared/queries';
import { AsanaCard } from '../home/integrations/AsanaCard';
import { NotionCard } from '../home/integrations/NotionCard';
import type { AiStatus } from '../shared/types';

const reducedMotion =
  typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

function Cat({ look, skin, pose }: { look: Species; skin: SkinId; pose: CatPose }) {
  return (
    <CatArt
      look={look}
      skin={skin}
      parts={pose.parts}
      motion={reducedMotion ? null : pose.motion}
      motionMs={pose.ms}
      motionLoop={pose.loop}
      mirrored={false}
      reducedMotion={reducedMotion}
    />
  );
}

/* ---------- 1 · Your pet ---------- */

export function PetStep(props: {
  name: string;
  onName: (name: string) => void;
  look: Species;
  onLook: (look: Species) => void;
  skin: SkinId;
}) {
  const t = useT();
  const styleId = useId();
  return (
    <div className="ob-body">
      <div className="ob-stage">
        <div className="ob-stage__cat"><Cat look={props.look} skin={props.skin} pose={poseFor('idle')} /></div>
      </div>
      <div className="ob-form">
        <div>
          <h1 className="display ob-h1">{t('onboarding.pet.title')}</h1>
          <p className="ob-lead">{t('onboarding.pet.body')}</p>
        </div>
        <div>
          <span className="ob-k" id={styleId}>{t('petroom.species')}</span>
          <SpeciesTiles value={props.look} skin={props.skin} onChange={props.onLook} labelledBy={styleId} />
        </div>
        <div>
          <label className="ob-k" htmlFor="ob-name">{t('onboarding.pet.name')}</label>
          <input
            id="ob-name"
            className="st-input ob-name"
            value={props.name}
            maxLength={24}
            autoComplete="off"
            onChange={(e) => props.onName(e.target.value)}
          />
        </div>
      </div>
    </div>
  );
}

/* ---------- shared layout for steps 2–5 ---------- */

function SideLayout(props: { look: Species; skin: SkinId; pose: CatPose; title: string; lead?: string; children: ReactNode }) {
  return (
    <div className="ob-pad">
      <div className="ob-side">
        <div className="ob-side__cat"><Cat look={props.look} skin={props.skin} pose={props.pose} /></div>
      </div>
      <div className="ob-main">
        <div>
          <h1 className="ob-h1--sm">{props.title}</h1>
          {props.lead ? <p className="ob-lead">{props.lead}</p> : null}
        </div>
        {props.children}
      </div>
    </div>
  );
}

/* ---------- 2 · Permissions ---------- */

export function PermissionsStep({ look, skin }: { look: Species; skin: SkinId }) {
  const t = useT();
  const qc = useQueryClient();
  const perms = useQuery({ queryKey: ['permissions'], queryFn: permissionsStatus, refetchInterval: 2000 });
  const cards = [
    { kind: 'screenRecording', icon: 'screenshot', title: 'onboarding.perm.screen', desc: 'onboarding.perm.screenDesc' },
    { kind: 'accessibility', icon: 'paste', title: 'onboarding.perm.accessibility', desc: 'onboarding.perm.accessibilityDesc' },
    { kind: 'microphone', icon: 'mic', title: 'onboarding.perm.mic', desc: 'onboarding.perm.micDesc' },
  ] as const;
  return (
    <SideLayout look={look} skin={skin} pose={poseFor('thinking')} title={t('onboarding.perm.title')} lead={t('onboarding.perm.body')}>
      {cards.map((c) => {
        const value = perms.data?.[c.kind];
        const granted = value === true || value === 'granted';
        // macOS asks for the microphone itself the first time Co-pet records.
        const askLater = value === 'notDetermined';
        return (
          <div key={c.kind} className="ob-perm">
            <div className="ob-perm__icon"><Icon name={c.icon} size={20} /></div>
            <div className="ob-perm__text"><b>{t(c.title)}</b>{t(c.desc)}</div>
            <div className="ob-perm__side">
              {granted ? (
                <StatusPill status="synced" label={t('onboarding.perm.allowed')} />
              ) : askLater ? (
                <StatusPill status="pending" label={t('onboarding.perm.askLater')} />
              ) : (
                <>
                  <StatusPill status="pending" label={t('onboarding.perm.notAllowed')} />
                  <Button
                    size="sm"
                    onClick={() =>
                      void requestPermission(c.kind).finally(
                        () => void qc.invalidateQueries({ queryKey: ['permissions'] }),
                      )
                    }
                  >
                    {t('onboarding.perm.open')}
                  </Button>
                </>
              )}
            </div>
          </div>
        );
      })}
    </SideLayout>
  );
}

/* ---------- 3 · Local AI ---------- */

export function AiStep(props: { look: Species; skin: SkinId; status: AiStatus | undefined; localOnly: boolean }) {
  const t = useT();
  const lang = useLang();
  const run = useAiAction();
  const labelId = useId();
  const { status, localOnly } = props;
  const selected = status ? status.modelId ?? status.recommendedId : null;
  const selectedModel = status?.models.find((m) => m.id === selected);
  const ready = status?.state === 'ready' && !!selectedModel?.downloaded;

  const choose = (id: string) => {
    if (id === selected) return;
    void run(async () => {
      await aiSelectModel(id);
      if (!localOnly && status?.state !== 'none') await aiDownload(id);
    });
  };

  return (
    <SideLayout look={props.look} skin={props.skin} pose={poseFor('idle')} title={t('onboarding.ai.title')} lead={t('onboarding.ai.body')}>
      <div>
        <span className="ob-k" id={labelId}>{t('onboarding.ai.choose')}</span>
        <div className="ob-models" role="radiogroup" aria-labelledby={labelId}>
          {status?.models.map((m) => (
            <button
              key={m.id}
              type="button"
              role="radio"
              aria-checked={m.id === selected}
              className="ob-model"
              onClick={() => choose(m.id)}
            >
              <b>{modelName(m)}</b>
              <span>{modelMeta(m, t, lang)}</span>
              {m.id === status?.recommendedId ? (
                <StatusPill status="local" icon="idea" label={t('settings.triage.recommended')} />
              ) : null}
            </button>
          ))}
        </div>
      </div>
      {isWindows ? (
        <div>
          <span className="ob-k">{t('storage.onboardingTitle')}</span>
          <StorageRows mode="onboarding" />
        </div>
      ) : null}
      <p className="st-info">
        <Icon name={ready ? 'check' : 'idea'} size={20} />
        <span>
          {localOnly
            ? t('settings.triage.localOnlyBlocked')
            : ready
              ? t('ai.ready')
              : t('ai.rulesUntilDownload', { size: selectedModel ? formatSize(selectedModel.bytes, lang) : '' })}
        </span>
      </p>
      <p className="caption ob-caption">{t('onboarding.ai.cloudLater')}</p>
    </SideLayout>
  );
}

/* ---------- 4 · Apps ---------- */

/**
 * Optional Notion / Asana connection (the Integrations cards without target pickers). An OAuth
 * sign-in finishing now only emits `integration-changed`, which refreshes these cards; Home
 * stays closed until onboarding ends. Continue skips.
 */
export function AppsStep({ look, skin }: { look: Species; skin: SkinId }) {
  const t = useT();
  const { data: statuses } = useIntegrations();
  return (
    <SideLayout look={look} skin={skin} pose={poseFor('carrying')} title={t('onboarding.apps.title')}>
      <NotionCard status={statuses?.find((s) => s.provider === 'notion')} compact />
      <AsanaCard status={statuses?.find((s) => s.provider === 'asana')} compact />
      <p className="caption ob-caption">{t('onboarding.apps.caption')}</p>
    </SideLayout>
  );
}

/* ---------- 5 · Hotkeys ---------- */

export function HotkeysStep({ look, skin }: { look: Species; skin: SkinId }) {
  const t = useT();
  return (
    <SideLayout look={look} skin={skin} pose={poseFor('gesture')} title={t('onboarding.hotkeys.title')} lead={t('onboarding.hotkeys.body')}>
      <HotkeyRows />
    </SideLayout>
  );
}

/* ---------- download strip (steps 2–5) ---------- */

export function DownloadStrip({ status }: { status: AiStatus }) {
  const t = useT();
  const lang = useLang();
  const run = useAiAction();
  const transfer = transferOf(status, status.modelId ?? '');
  if (!transfer && status.state !== 'ready') return null;
  const id = status.modelId ?? '';
  const pct = downloadPercent(status);
  const size = formatSize(status.bytesTotal, lang);

  let line: string;
  let action: { label: string; icon?: 'pause' | 'play'; fn: () => Promise<void> } | null = null;
  switch (transfer) {
    case 'downloading':
      line = t('onboarding.dl.downloading', { size, percent: pct });
      action = { label: t('common.pause'), icon: 'pause', fn: aiPause };
      break;
    case 'paused':
      line = t('onboarding.dl.paused', { size, percent: pct });
      action = { label: t('common.resume'), icon: 'play', fn: () => aiDownload(id) };
      break;
    case 'verifying':
      line = t('settings.triage.verifying');
      break;
    case 'failed':
      line = t('settings.triage.failed', { reason: aiErrorReason(t, status.error) });
      action = { label: t('common.retry'), fn: () => aiDownload(id) };
      break;
    default:
      line = t('ai.ready');
  }
  const showBar = transfer === 'downloading' || transfer === 'paused' || transfer === 'verifying';
  const act = action;

  return (
    <div className="ob-dl" role="status">
      <Icon name={transfer ? 'idea' : 'check'} size={20} />
      <div className="ob-dl__text">
        <b>{line}</b>
        {transfer === 'downloading' || transfer === 'paused' ? <> {t('onboarding.dl.resumes')}</> : null}
        {showBar ? (
          <ProgressBar
            value={transfer === 'verifying' ? null : pct / 100}
            label={t('settings.triage.progressLabel', { percent: pct })}
          />
        ) : null}
      </div>
      {act ? (
        <Button variant="quiet" size="sm" icon={act.icon} onClick={() => void run(act.fn)}>
          {act.label}
        </Button>
      ) : null}
    </div>
  );
}
