import { useEffect } from 'react'

import { useProfiles } from '@/hooks/use-profiles'
import { useProxiesData } from '@/providers/app-data-context'
import speedManager, { speedIdentityOfNode } from '@/services/speed'

/** 把当前订阅作用域同步给 SpeedManager。
 *
 * - 切换订阅:清空会话缓存并载入该订阅的持久化结果,展示随订阅隔离。
 * - 订阅刷新:节点集合成功加载后清理该订阅内已不存在的旧结果;空集合视为
 *   加载不完整而跳过,避免瞬时刷新误删。
 */
export function useSpeedProfileScope() {
  const { current } = useProfiles()
  const { proxyView } = useProxiesData()
  const profileUid = current?.uid ?? null

  useEffect(() => {
    speedManager.setProfileScope(profileUid)
  }, [profileUid])

  useEffect(() => {
    if (!profileUid || !proxyView) return
    const identities = new Set(
      Object.values(proxyView.records ?? {}).map((node) =>
        speedIdentityOfNode(node),
      ),
    )
    if (identities.size === 0) return
    speedManager.pruneProfile(profileUid, identities)
  }, [profileUid, proxyView])
}
