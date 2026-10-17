// Pet room: a large preview stage with animation chips, and the pet's name, species, skin and size.
import { useEffect, useId, useState, type ReactElement } from 'react';
import { SegmentedControl, Switch } from '../../ds';
import { asPetSize, petLookOf } from '../../pet/catalog';
import { CatArt } from '../../pet/CatArt';
import { fxForArt, poseFor } from '../../pet/pose';
import { usePetLook } from '../../pet/usePetMachine';
import { useT } from '../../shared/i18n';
import { useSetSetting, useSettings } from '../../shared/queries';
import { useMediaQuery } from '../hooks';
import { navigate } from '../router';
import { SkinSwatches, SpeciesTiles } from '../settings/parts';
import './petroom.css';

const NAME_MAX = 24;
const LOOPS = ['idle', 'thinking', 'sleep', 'holdSign'] as const;
const ONE_SHOTS = ['catch', 'idea', 'celebrate', 'confused'] as const;
type Loop = (typeof LOOPS)[number];
type OneShot = (typeof ONE_SHOTS)[number];

export function PetRoomPage(): ReactElement {
  const { data: settings } = useSettings();
  const set = useSetSetting();
  const t = useT();
  const look = petLookOf(settings);
  const size = asPetSize(settings?.['pet.size']);
  const speaks = settings?.['pet.speaks'] === true;
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)');
  const { machine, output, applied } = usePetLook(settings ? look : null, settings ? `${look.species}/${look.skin}` : null);
  const [loop, setLoop] = useState<Loop>('idle');
  const pickLoop = (l: Loop) => {
    for (const f of ['thinking', 'sleep', 'holdSign'] as const) machine.set(f, f === l);
    setLoop(l);
  };
  const play = (trigger: OneShot) => {
    pickLoop('idle');
    machine.fire(trigger);
  };

  const savedName = typeof settings?.['pet.name'] === 'string' && settings['pet.name'] ? settings['pet.name'] : 'Miso';
  const [name, setName] = useState(savedName);
  useEffect(() => {
    setName(savedName);
  }, [savedName]);
  const commitName = () => {
    const next = name.trim();
    if (!next) setName(savedName);
    else if (next !== savedName) void set('pet.name', next);
  };

  const speciesId = useId();
  const skinId = useId();
  const pose = poseFor(output.body, output.variant);

  return (
    <div className="pr-page">
      <section className="pr-stage" aria-label={t('petroom.preview')}>
        <div className="pr-stage__pet">
          {applied && (
            <CatArt
              look={applied.species}
              skin={applied.skin}
              parts={pose.parts}
              motion={pose.motion}
              motionMs={pose.ms}
              motionKey={output.seq}
              motionLoop={pose.loop}
              mirrored={false}
              reducedMotion={reducedMotion}
              fx={fxForArt(output.fx)}
            />
          )}
        </div>
        <div className="pr-poses" role="group" aria-label={t('petroom.previewAnimations')}>
          {LOOPS.map((id) => (
            <button key={id} type="button" className="pr-pose" aria-pressed={loop === id} onClick={() => pickLoop(id)}>
              {t(`petroom.pose.${id}`)}
            </button>
          ))}
          {ONE_SHOTS.map((id) => (
            <button key={id} type="button" className="pr-pose" onClick={() => play(id)}>
              {t(`petroom.pose.${id}`)}
            </button>
          ))}
        </div>
      </section>
      <aside className="pr-side" aria-label={t('petroom.customize')}>
        <div>
          <label className="pr-k" htmlFor="pr-name">{t('petroom.name')}</label>
          <input
            id="pr-name"
            className="st-input pr-name"
            value={name}
            maxLength={NAME_MAX}
            onChange={(e) => setName(e.target.value)}
            onBlur={commitName}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitName();
              if (e.key === 'Escape') setName(savedName);
            }}
          />
        </div>
        <div>
          <span className="pr-k" id={speciesId}>{t('petroom.species')}</span>
          <SpeciesTiles value={look.species} skin={look.skin} onChange={(v) => void set('pet.style', v)} labelledBy={speciesId} />
        </div>
        <div>
          <span className="pr-k" id={skinId}>{t('petroom.skin')}</span>
          <SkinSwatches species={look.species} value={look.skin} onChange={(v) => void set('pet.skin', v)} labelledBy={skinId} />
          <p className="pr-cap">{t('petroom.skinCaption')}</p>
        </div>
        <div>
          <span className="pr-k">{t('petroom.size')}</span>
          <SegmentedControl
            label={t('petroom.size')}
            value={String(size)}
            onChange={(v) => void set('pet.size', Number(v))}
            options={[
              { value: '64', label: t('petroom.sizeSmall') },
              { value: '80', label: t('petroom.sizeMedium') },
              { value: '96', label: t('petroom.sizeLarge') },
            ]}
          />
        </div>
        <Switch
          id="pr-speaks"
          label={t('petroom.speaks')}
          description={t('petroom.speaksDesc')}
          checked={speaks}
          onChange={(v) => void set('pet.speaks', v)}
        />
        <p className="pr-behaviour">
          {t('petroom.behaviour')}{' '}
          <button type="button" className="st-link" onClick={() => navigate('settings/notifications')}>
            {t('petroom.openSettings')}
          </button>
        </p>
      </aside>
    </div>
  );
}
