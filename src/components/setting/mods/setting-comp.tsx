import { ChevronRightRounded } from '@mui/icons-material'
import {
  Box,
  List,
  ListItem,
  ListItemButton,
  ListItemText,
  ListSubheader,
} from '@mui/material'
import CircularProgress from '@mui/material/CircularProgress'
import React, { ReactNode, useState } from 'react'

import isAsyncFunction from '@/utils/is-async-function'

interface ItemProps {
  'data-setting-target'?: string
  label: ReactNode
  extra?: ReactNode
  children?: ReactNode
  secondary?: ReactNode
  /** 纵向布局:标签在上、控件在下(用于窄控件/长标签)。 */
  vertical?: boolean
  onClick?: () => void | Promise<any>
}

export const SettingItem: React.FC<ItemProps> = ({
  'data-setting-target': searchTarget,
  label,
  extra,
  children,
  secondary,
  vertical = false,
  onClick,
}) => {
  const clickable = !!onClick

  const primary = (
    <Box sx={{ display: 'flex', alignItems: 'center', fontSize: '14px' }}>
      <span>{label}</span>
      {extra ? extra : null}
    </Box>
  )

  const [isLoading, setIsLoading] = useState(false)
  const handleClick = () => {
    if (onClick) {
      if (isAsyncFunction(onClick)) {
        setIsLoading(true)
        onClick()!.finally(() => setIsLoading(false))
      } else {
        onClick()
      }
    }
  }

  return clickable ? (
    <ListItem data-setting-target={searchTarget} disablePadding>
      <ListItemButton onClick={handleClick} disabled={isLoading}>
        <ListItemText primary={primary} secondary={secondary} />
        {isLoading ? (
          <CircularProgress color="inherit" size={20} />
        ) : (
          <ChevronRightRounded />
        )}
      </ListItemButton>
    </ListItem>
  ) : vertical ? (
    <ListItem
      data-setting-target={searchTarget}
      sx={{
        pt: '5px',
        pb: '5px',
        flexDirection: 'column',
        alignItems: 'stretch',
        gap: 1,
      }}
    >
      <ListItemText primary={primary} secondary={secondary} />
      {children}
    </ListItem>
  ) : (
    <ListItem data-setting-target={searchTarget} sx={{ pt: '5px', pb: '5px' }}>
      <ListItemText primary={primary} secondary={secondary} />
      {children}
    </ListItem>
  )
}

export const SettingList: React.FC<{
  title: string
  children: ReactNode
}> = ({ title, children }) => (
  <List>
    <ListSubheader
      sx={[
        { background: 'transparent', fontSize: '16px', fontWeight: '700' },
        ({ palette }) => {
          return {
            color: palette.text.primary,
          }
        },
      ]}
      disableSticky
    >
      {title}
    </ListSubheader>

    {children}
  </List>
)
