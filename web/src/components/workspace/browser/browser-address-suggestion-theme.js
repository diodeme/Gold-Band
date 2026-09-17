// Applied at document start inside the address suggestion webview, before the surface
// renders. Without it the first paint uses the stylesheet default (dark), which shows up as
// a black flash before the live theme is applied.
(function (theme) {
  if (!theme) return;
  var root = document.documentElement;
  root.classList.toggle('dark', Boolean(theme.dark));
  if (theme.themeId) root.dataset.theme = theme.themeId;
  if (theme.colorScheme) root.dataset.colorScheme = theme.colorScheme;
  if (theme.visualQuality) root.dataset.visualQuality = theme.visualQuality;
  if (theme.materialModel) root.dataset.materialModel = theme.materialModel;
  root.style.colorScheme = theme.dark ? 'dark' : 'light';
  var variables = theme.variables || {};
  Object.keys(variables).forEach(function (name) {
    root.style.setProperty(name, variables[name]);
  });
})
