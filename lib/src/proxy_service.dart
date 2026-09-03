import 'rust/api/local_proxy.dart' as rust_api;
import 'rust/frb_generated.dart';

abstract interface class ProxyService {
  List<rust_api.ProfileSummary> listProfiles();

  rust_api.ProfileView getProfile(String id);

  rust_api.AppSettingsView getSettings();

  rust_api.AppSettingsView saveSettings(rust_api.AppSettingsDraft settings);

  rust_api.ProfileView saveProfile(rust_api.ProfileDraft draft);

  void deleteProfile(String id);

  rust_api.StartOutcome startProfile(String id);

  void stopProfile(String id);

  void confirmHostKey(String id, String fingerprint, bool accepted);

  Stream<rust_api.ProxyEvent> events();
}

class RustProxyService implements ProxyService {
  const RustProxyService();

  @override
  List<rust_api.ProfileSummary> listProfiles() => rust_api.listProfiles();

  @override
  rust_api.ProfileView getProfile(String id) => rust_api.getProfile(id: id);

  @override
  rust_api.AppSettingsView getSettings() => rust_api.getSettings();

  @override
  rust_api.AppSettingsView saveSettings(rust_api.AppSettingsDraft settings) =>
      rust_api.saveSettings(settings: settings);

  @override
  rust_api.ProfileView saveProfile(rust_api.ProfileDraft draft) =>
      rust_api.saveProfile(draft: draft);

  @override
  void deleteProfile(String id) => rust_api.deleteProfile(id: id);

  @override
  rust_api.StartOutcome startProfile(String id) =>
      rust_api.startProfile(id: id);

  @override
  void stopProfile(String id) => rust_api.stopProfile(id: id);

  @override
  void confirmHostKey(String id, String fingerprint, bool accepted) => rust_api
      .confirmHostKey(id: id, fingerprint: fingerprint, accepted: accepted);

  @override
  Stream<rust_api.ProxyEvent> events() => rust_api.events();
}

Future<void> initializeRust() => RustLib.init();
