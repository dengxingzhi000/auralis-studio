/**
 * NSIS 自定义卸载钩子。
 *
 * 静默卸载测试暴露了一个问题：卸载程序删除了安装目录，但在某些 Windows
 * 环境下会在删除快捷方式/注册表之前退出，留下开始菜单项与卸载注册表键。
 * 这里在文件删除**之前**显式清理这两个状态，正常卸载与升级都能重复执行。
 *
 * 升级（isUpdated）时保留快捷方式与注册表，交给新安装程序接管。
 */
!macro customUnInstall
  ${ifNot} ${isUpdated}
    Delete "$SMPROGRAMS\${SHORTCUT_NAME}.lnk"
    Delete "$DESKTOP\${SHORTCUT_NAME}.lnk"
    DeleteRegKey SHELL_CONTEXT "${UNINSTALL_REGISTRY_KEY}"
    DeleteRegKey SHELL_CONTEXT "${INSTALL_REGISTRY_KEY}"
  ${endif}
!macroend
