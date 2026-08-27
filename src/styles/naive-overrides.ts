/**
 * Naive UI 全局 themeOverrides
 * 收敛组件密度与圆角，对齐 Desktop Skin §3.2。
 * 色值引用 tokens.scss 同值常量，不复制粘贴 hex。
 */

import type { GlobalThemeOverrides } from "naive-ui";

// ─── 色值常量（与 tokens.scss 同步） ───────────────────────
const LIGHT = {
  primary: "#3370ff",
  primaryHover: "#5a8fff",
  border: "#e0e1e3",
  bgApp: "#f5f6f7",
  bgPanel: "#ffffff",
  bgHover: "#f0f1f2",
  text: "#1f2329",
  textDim: "#8f959e",
};

const DARK = {
  primary: "#4d8bf5",
  primaryHover: "#6ea0f7",
  border: "#3b3b3b",
  bgApp: "#1e1e1e",
  bgPanel: "#252526",
  bgHover: "#2a2d2e",
  text: "#cccccc",
  textDim: "#808080",
};

/** 亮色 themeOverrides */
export const lightOverrides: GlobalThemeOverrides = {
  common: {
    borderRadius: "4px",
    borderRadiusSmall: "2px",
    fontSize: "13px",
    fontSizeMini: "11px",
    fontSizeTiny: "11px",
    fontSizeSmall: "12px",
    fontSizeMedium: "13px",
    fontSizeLarge: "14px",
    primaryColor: LIGHT.primary,
    primaryColorHover: LIGHT.primaryHover,
    borderColor: LIGHT.border,
    textColorBase: LIGHT.text,
    textColor1: LIGHT.text,
    textColor2: LIGHT.text,
    textColor3: LIGHT.textDim,
    hoverColor: LIGHT.bgHover,
    cardColor: LIGHT.bgPanel,
    modalColor: LIGHT.bgPanel,
    popoverColor: LIGHT.bgPanel,
    tableColor: LIGHT.bgPanel,
    inputColor: LIGHT.bgPanel,
  },
  Button: {
    heightSmall: "28px",
    heightMedium: "32px",
    heightLarge: "36px",
    fontSizeSmall: "12px",
    fontSizeMedium: "13px",
    paddingSmall: "0 10px",
    paddingMedium: "0 14px",
  },
  Input: {
    heightSmall: "28px",
    heightMedium: "32px",
    heightLarge: "36px",
    fontSizeSmall: "12px",
    fontSizeMedium: "13px",
    paddingSmall: "0 8px",
    paddingMedium: "0 10px",
  },
  Select: {
    heightSmall: "28px",
    heightMedium: "32px",
    heightLarge: "36px",
    fontSizeSmall: "12px",
    fontSizeMedium: "13px",
  },
  DataTable: {
    borderRadius: "4px",
    fontSizeSmall: "12px",
    fontSizeMedium: "13px",
    thFontSizeSmall: "12px",
    thFontSizeMedium: "13px",
    tdPaddingSmall: "6px 8px",
    tdPaddingMedium: "8px 12px",
    thPaddingSmall: "6px 8px",
    thPaddingMedium: "8px 12px",
  },
  Card: {
    borderRadius: "4px",
    paddingSmall: "12px",
    paddingMedium: "16px",
    paddingLarge: "20px",
    fontSize: "13px",
  },
  Dialog: {
    borderRadius: "4px",
    padding: "16px",
    fontSize: "13px",
  },
  Tag: {
    borderRadius: "2px",
    fontSizeSmall: "11px",
    fontSizeMedium: "12px",
    heightSmall: "20px",
    heightMedium: "24px",
  },
  Tabs: {
    tabFontSizeSmall: "12px",
    tabFontSizeMedium: "13px",
    tabGapSmall: "12px",
    tabGapMedium: "16px",
  },
  Menu: {
    borderRadius: "4px",
    fontSize: "13px",
    itemHeight: "32px",
  },
  Dropdown: {
    borderRadius: "4px",
    fontSize: "13px",
    optionHeightSmall: "28px",
    optionHeightMedium: "32px",
  },
  Message: {
    borderRadius: "4px",
    fontSize: "13px",
  },
  Notification: {
    borderRadius: "4px",
    fontSize: "13px",
  },
};

/** 暗色 themeOverrides（仅覆盖色值，尺寸/圆角继承亮色） */
export const darkOverrides: GlobalThemeOverrides = {
  common: {
    borderRadius: "4px",
    borderRadiusSmall: "2px",
    fontSize: "13px",
    primaryColor: DARK.primary,
    primaryColorHover: DARK.primaryHover,
    borderColor: DARK.border,
    textColorBase: DARK.text,
    textColor1: DARK.text,
    textColor2: DARK.text,
    textColor3: DARK.textDim,
    hoverColor: DARK.bgHover,
    cardColor: DARK.bgPanel,
    modalColor: DARK.bgPanel,
    popoverColor: DARK.bgPanel,
    tableColor: DARK.bgPanel,
    inputColor: DARK.bgPanel,
  },
  Button: {
    heightSmall: "28px",
    heightMedium: "32px",
    heightLarge: "36px",
  },
  Input: {
    heightSmall: "28px",
    heightMedium: "32px",
    heightLarge: "36px",
  },
  Select: {
    heightSmall: "28px",
    heightMedium: "32px",
    heightLarge: "36px",
  },
  DataTable: {
    borderRadius: "4px",
  },
  Card: {
    borderRadius: "4px",
  },
  Dialog: {
    borderRadius: "4px",
  },
  Tag: {
    borderRadius: "2px",
  },
  Tabs: {},
  Menu: {
    borderRadius: "4px",
  },
  Dropdown: {
    borderRadius: "4px",
  },
  Message: {
    borderRadius: "4px",
  },
  Notification: {
    borderRadius: "4px",
  },
};
