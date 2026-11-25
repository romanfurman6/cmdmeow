import React from 'react';
import {Composition} from 'remotion';
import './fonts.css';
import {Promo16x9} from './Promo16x9';
import {Promo9x16} from './Promo9x16';
import {FPS, FRAMES} from './timing';
import {PromoV2, V2_DURATION} from './PromoV2';

export const RemotionRoot: React.FC = () => (
  <>
    <Composition id="Promo16x9" component={Promo16x9} durationInFrames={FRAMES} fps={FPS} width={1920} height={1080} />
    <Composition id="Promo9x16" component={Promo9x16} durationInFrames={FRAMES} fps={FPS} width={1080} height={1920} />
    <Composition id="PromoV2" component={PromoV2} durationInFrames={Math.round(V2_DURATION * 30)} fps={30} width={1920} height={1080} />
  </>
);
