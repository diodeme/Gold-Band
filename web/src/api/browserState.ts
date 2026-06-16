import type { AppBootstrapVm, AutoTemplateStore, PreferencesVm, ProfileListVm, ProfileVm, UpdateBadgeStateVm, UpdateStatusVm, UpdaterSettingsVm, WorkflowTemplateStore } from '../types';
import { mockBootstrap, mockProfileList, mockUpdateBadges, mockUpdateStatus, mockUpdaterSettings, mockWorkflowTemplates } from '../mockData';

export class BrowserPreviewState {
  private profiles: ProfileVm[] = cloneProfiles(mockProfileList.profiles);
  private preferences: PreferencesVm = clonePreferences(mockBootstrap.preferences);
  private updaterSettings: UpdaterSettingsVm = cloneUpdaterSettings(mockUpdaterSettings);
  private updateStatus: UpdateStatusVm = cloneUpdateStatus(mockUpdateStatus);
  private updateBadges: UpdateBadgeStateVm = cloneUpdateBadges(mockUpdateBadges);
  private workflowTemplates: WorkflowTemplateStore = cloneWorkflowTemplateStore(mockWorkflowTemplates);
  private autoTemplates: AutoTemplateStore = { version: '0.1', templates: [] };

  getAppBootstrap(): AppBootstrapVm {
    return {
      ...mockBootstrap,
      preferences: this.getPreferences(),
      updaterSettings: this.getUpdaterSettings(),
      updateStatus: this.getUpdateStatus(),
      updateBadges: this.getUpdateBadges(),
      persistedAvailableUpdate: this.updateStatus.update ?? null,
      clientVersion: mockBootstrap.clientVersion,
    };
  }

  getPreferences(): PreferencesVm {
    return clonePreferences(this.preferences);
  }

  setPreferences(preferences: PreferencesVm) {
    this.preferences = clonePreferences(preferences);
    return this.getPreferences();
  }

  getProfiles(): ProfileListVm {
    return { profiles: cloneProfiles(this.profiles) };
  }

  getProfile(id: string): ProfileVm | undefined {
    const profile = this.profiles.find((item) => item.id === id);
    return profile ? cloneProfile(profile) : undefined;
  }

  addProfile(profile: ProfileVm) {
    this.profiles = [...this.profiles, cloneProfile(profile)];
    return cloneProfile(profile);
  }

  updateProfile(profile: ProfileVm) {
    this.profiles = this.profiles.map((item) => item.id === profile.id ? cloneProfile(profile) : item);
    return cloneProfile(profile);
  }

  removeProfile(id: string) {
    this.profiles = this.profiles.filter((item) => item.id !== id);
    return this.getProfiles();
  }

  getUpdaterSettings(): UpdaterSettingsVm {
    return cloneUpdaterSettings(this.updaterSettings);
  }

  setUpdaterSettings(settings: UpdaterSettingsVm) {
    this.updaterSettings = cloneUpdaterSettings(settings);
    return this.getUpdaterSettings();
  }

  getUpdateStatus(): UpdateStatusVm {
    return cloneUpdateStatus(this.updateStatus);
  }

  setUpdateStatus(status: UpdateStatusVm) {
    this.updateStatus = cloneUpdateStatus(status);
    return this.getUpdateStatus();
  }

  getUpdateBadges(): UpdateBadgeStateVm {
    return cloneUpdateBadges(this.updateBadges);
  }

  setUpdateBadges(badges: UpdateBadgeStateVm) {
    this.updateBadges = cloneUpdateBadges(badges);
    return this.getUpdateBadges();
  }

  getWorkflowTemplates(): WorkflowTemplateStore {
    return cloneWorkflowTemplateStore(this.workflowTemplates);
  }

  setWorkflowTemplates(store: WorkflowTemplateStore) {
    this.workflowTemplates = cloneWorkflowTemplateStore(store);
    return this.getWorkflowTemplates();
  }

  getAutoTemplates(): AutoTemplateStore {
    return cloneAutoTemplateStore(this.autoTemplates);
  }

  setAutoTemplates(store: AutoTemplateStore) {
    this.autoTemplates = cloneAutoTemplateStore(store);
    return this.getAutoTemplates();
  }
}

export const browserPreviewState = new BrowserPreviewState();

function cloneProfile(profile: ProfileVm): ProfileVm {
  return { ...profile };
}

function cloneProfiles(profiles: ProfileVm[]): ProfileVm[] {
  return profiles.map(cloneProfile);
}

function clonePreferences(preferences: PreferencesVm): PreferencesVm {
  return { ...preferences };
}

function cloneUpdaterSettings(settings: UpdaterSettingsVm): UpdaterSettingsVm {
  return { ...settings };
}

function cloneUpdateStatus(status: UpdateStatusVm): UpdateStatusVm {
  return {
    ...status,
    update: status.update ? { ...status.update } : status.update ?? null,
    error: status.error ? { code: status.error.code, params: { ...status.error.params } } : status.error ?? null,
  };
}

function cloneUpdateBadges(badges: UpdateBadgeStateVm): UpdateBadgeStateVm {
  return { ...badges };
}

function cloneWorkflowTemplateStore(store: WorkflowTemplateStore): WorkflowTemplateStore {
  return {
    ...store,
    lastCreatedWorkflow: store.lastCreatedWorkflow ? structuredClone(store.lastCreatedWorkflow) : null,
    templates: store.templates.map((template) => ({
      ...template,
      workflow: structuredClone(template.workflow),
    })),
  };
}

function cloneAutoTemplateStore(store: AutoTemplateStore): AutoTemplateStore {
  return {
    ...store,
    templates: store.templates.map((template) => ({
      ...template,
      config: structuredClone(template.config),
    })),
  };
}
