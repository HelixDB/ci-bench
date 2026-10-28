import { REGION } from '../src/setup-env'

;(globalThis as { __ADV_REGION__?: string }).__ADV_REGION__ = REGION
