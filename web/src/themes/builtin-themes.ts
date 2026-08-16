import generatedCatalog from './generated/catalog.json';

import { themePackageSchema, type ThemePackage } from '../theme-contract';

export const builtinThemes: readonly ThemePackage[] = themePackageSchema.array().parse(generatedCatalog);
