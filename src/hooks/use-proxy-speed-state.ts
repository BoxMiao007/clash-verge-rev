import { useLockFn } from 'ahooks'
import { useCallback, useEffect, useReducer } from 'react'

import speedManager, { type SpeedUpdate } from '@/services/speed'
import {
  isInteractableMember,
  type ResolvedProxyMember,
} from '@/types/proxy-view'
import {
  SPEED_TESTING,
  DEFAULT_SPEEDTEST_URL,
  DEFAULT_SPEEDTEST_WINDOW_SECS,
} from '@/utils/speed'

const PRESET_PROXY_NAMES = [
  'DIRECT',
  'REJECT',
  'REJECT-DROP',
  'PASS',
  'COMPATIBLE',
]

const identity = (_: SpeedUpdate, next: SpeedUpdate): SpeedUpdate => next

const INITIAL_SPEED: SpeedUpdate = { speed: -1, updatedAt: 0 }

export interface UseProxySpeedState {
  speedState: SpeedUpdate
  speedValue: number
  isPreset: boolean
  onSpeed: () => Promise<void>
}

export function useProxySpeedState(
  member: ResolvedProxyMember,
  groupName: string,
): UseProxySpeedState {
  const name = member.ref.name
  const unresolved = member.kind === 'unresolved'
  const isPreset = unresolved || PRESET_PROXY_NAMES.includes(name)
  const [speedState, setSpeedState] = useReducer(identity, INITIAL_SPEED)

  useEffect(() => {
    if (isPreset) return
    speedManager.setListener(name, groupName, setSpeedState)
    return () => {
      speedManager.removeListener(name, groupName)
    }
  }, [name, groupName, isPreset])

  const updateSpeed = useCallback(() => {
    if (unresolved) {
      setSpeedState(INITIAL_SPEED)
      return
    }
    const cachedUpdate = speedManager.getSpeedUpdate(name, groupName)
    if (cachedUpdate) {
      setSpeedState({ ...cachedUpdate })
      return
    }

    setSpeedState(INITIAL_SPEED)
  }, [groupName, name, unresolved])

  useEffect(() => {
    updateSpeed()
  }, [updateSpeed])

  const onSpeed = useLockFn(async () => {
    if (!isInteractableMember(member)) return
    setSpeedState({ speed: SPEED_TESTING, updatedAt: Date.now() })
    setSpeedState(
      await speedManager.checkSpeed(
        member,
        groupName,
        DEFAULT_SPEEDTEST_URL,
        DEFAULT_SPEEDTEST_WINDOW_SECS,
      ),
    )
  })

  return {
    speedState,
    speedValue: speedState.speed,
    isPreset,
    onSpeed,
  }
}
