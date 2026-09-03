import 'dart:async';

import 'package:flutter/foundation.dart';

import 'rust/api/local_proxy.dart' as rust_api;
import 'proxy_service.dart';

class PendingHostKey {
  const PendingHostKey({
    required this.profileId,
    required this.fingerprint,
    required this.message,
  });

  final String profileId;
  final String fingerprint;
  final String message;
}

class ProfileController extends ChangeNotifier {
  ProfileController({ProxyService? service})
    : service = service ?? const RustProxyService();

  final ProxyService service;
  final List<String> logs = <String>[];
  List<rust_api.ProfileSummary> profiles = <rust_api.ProfileSummary>[];
  rust_api.AppSettingsView? settings;
  rust_api.ProfileView? selected;
  String? selectedId;
  String? errorMessage;
  PendingHostKey? pendingHostKey;
  bool isLoading = false;
  StreamSubscription<rust_api.ProxyEvent>? _eventsSubscription;

  void initialize() {
    try {
      _eventsSubscription ??= service.events().listen(
        _onEvent,
        onError: (Object error) {
          errorMessage = error.toString();
          notifyListeners();
        },
      );
      refresh();
    } catch (error) {
      errorMessage = _friendlyError(error);
      notifyListeners();
    }
  }

  void refresh() {
    _run(() {
      settings = service.getSettings();
      profiles = service.listProfiles();
      if (selectedId != null) {
        final selectedExists = profiles.any(
          (profile) => profile.id == selectedId,
        );
        if (!selectedExists) {
          selectedId = null;
          selected = null;
        } else {
          selected = service.getProfile(selectedId!);
        }
      }
    });
  }

  void select(String id) {
    _run(() {
      selectedId = id;
      selected = service.getProfile(id);
    });
  }

  void save(rust_api.ProfileDraft draft) {
    _run(() {
      selected = service.saveProfile(draft);
      selectedId = selected!.id;
      profiles = service.listProfiles();
    });
  }

  void saveSettings(rust_api.AppSettingsDraft draft) {
    _run(() {
      settings = service.saveSettings(draft);
    });
  }

  void start(String id) {
    _run(() {
      service.startProfile(id);
      selectedId = id;
      selected = service.getProfile(id);
      profiles = service.listProfiles();
    });
  }

  void stop(String id) {
    _run(() {
      service.stopProfile(id);
      selectedId = id;
      selected = service.getProfile(id);
      profiles = service.listProfiles();
    });
  }

  void remove(String id) {
    _run(() {
      service.deleteProfile(id);
      profiles = service.listProfiles();
      if (selectedId == id) {
        selectedId = null;
        selected = null;
      }
    });
  }

  void confirmHostKey(bool accepted) {
    final request = pendingHostKey;
    if (request == null) return;
    _run(() {
      service.confirmHostKey(request.profileId, request.fingerprint, accepted);
      pendingHostKey = null;
    });
  }

  void clearError() {
    errorMessage = null;
    notifyListeners();
  }

  @override
  void dispose() {
    _eventsSubscription?.cancel();
    super.dispose();
  }

  void _onEvent(rust_api.ProxyEvent event) {
    final index = profiles.indexWhere(
      (profile) => profile.id == event.profileId,
    );
    if (index >= 0) {
      final old = profiles[index];
      final endpoints = event.endpoints.isEmpty
          ? old.endpoints
          : event.endpoints;
      profiles[index] = rust_api.ProfileSummary(
        id: old.id,
        name: old.name,
        tags: old.tags,
        kind: old.kind,
        state: event.state,
        endpoints: endpoints,
        error: event.state == rust_api.ServiceState.error
            ? event.message
            : null,
      );
    }
    if (event.kind == rust_api.EventKind.hostKey && event.fingerprint != null) {
      pendingHostKey = PendingHostKey(
        profileId: event.profileId,
        fingerprint: event.fingerprint!,
        message: event.message ?? '请确认 SSH 主机密钥',
      );
    }
    final message = event.message;
    if (message != null && message.isNotEmpty) {
      logs.insert(0, message);
      if (logs.length > 80) logs.removeLast();
    }
    if (selectedId == event.profileId) {
      try {
        selected = service.getProfile(event.profileId);
      } catch (_) {
        // The event already contains enough information to keep the list useful
        // while a profile is stopping or being deleted.
      }
    }
    notifyListeners();
  }

  void _run(void Function() action) {
    isLoading = true;
    errorMessage = null;
    notifyListeners();
    try {
      action();
    } catch (error) {
      errorMessage = _friendlyError(error);
    } finally {
      isLoading = false;
      notifyListeners();
    }
  }

  String _friendlyError(Object error) {
    final message = error.toString();
    return message.startsWith('Exception: ')
        ? message.substring('Exception: '.length)
        : message;
  }
}
