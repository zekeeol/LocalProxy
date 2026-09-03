import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:local_proxy/main.dart';
import 'package:local_proxy/src/profile_controller.dart';
import 'package:local_proxy/src/proxy_service.dart';
import 'package:local_proxy/src/rust/api/local_proxy.dart' as rust_api;

class _FakeProxyService implements ProxyService {
  _FakeProxyService(this._profiles)
    : _views = {
        for (final profile in _profiles)
          profile.id: rust_api.ProfileView(
            id: profile.id,
            name: profile.name,
            tags: profile.tags,
            kind: profile.kind,
            state: profile.state,
            endpoints: profile.endpoints,
            error: profile.error,
          ),
      };

  final List<rust_api.ProfileSummary> _profiles;
  final Map<String, rust_api.ProfileView> _views;
  final StreamController<rust_api.ProxyEvent> _events =
      StreamController<rust_api.ProxyEvent>.broadcast();
  String? confirmedProfileId;
  String? confirmedFingerprint;
  bool? confirmedAccepted;

  void emit(rust_api.ProxyEvent event) => _events.add(event);

  @override
  void confirmHostKey(String id, String fingerprint, bool accepted) {
    confirmedProfileId = id;
    confirmedFingerprint = fingerprint;
    confirmedAccepted = accepted;
  }

  @override
  void deleteProfile(String id) {}

  @override
  Stream<rust_api.ProxyEvent> events() => _events.stream;

  @override
  rust_api.AppSettingsView getSettings() => const rust_api.AppSettingsView();

  @override
  rust_api.ProfileView getProfile(String id) => _views[id]!;

  @override
  List<rust_api.ProfileSummary> listProfiles() => _profiles;

  @override
  rust_api.ProfileView saveProfile(rust_api.ProfileDraft draft) =>
      throw UnimplementedError();

  @override
  rust_api.AppSettingsView saveSettings(rust_api.AppSettingsDraft settings) =>
      rust_api.AppSettingsView(
        httpProxy: settings.httpProxy,
        socksProxy: settings.socksProxy,
      );

  @override
  rust_api.StartOutcome startProfile(String id) =>
      const rust_api.StartOutcome.started();

  @override
  void stopProfile(String id) {}

  Future<void> dispose() => _events.close();
}

void main() {
  test('maps state, endpoint, log, and host-key events', () async {
    const profile = rust_api.ProfileSummary(
      id: 'p1',
      name: '测试代理',
      tags: ['测试'],
      kind: rust_api.ProxyType.http,
      state: rust_api.ServiceState.stopped,
      endpoints: [],
    );
    final service = _FakeProxyService([profile]);
    final controller = ProfileController(service: service);
    controller.initialize();

    const endpoint = rust_api.Endpoint(
      kind: 'http',
      host: '127.0.0.1',
      port: 18080,
      description: 'HTTP',
    );
    service.emit(
      const rust_api.ProxyEvent(
        profileId: 'p1',
        kind: rust_api.EventKind.state,
        state: rust_api.ServiceState.running,
        message: 'HTTP 反向代理已启动',
        endpoints: [endpoint],
      ),
    );
    await Future<void>.delayed(Duration.zero);

    expect(controller.profiles.single.state, rust_api.ServiceState.running);
    expect(controller.profiles.single.tags, ['测试']);
    expect(controller.profiles.single.endpoints.single.port, 18080);
    expect(controller.logs.single, 'HTTP 反向代理已启动');

    service.emit(
      const rust_api.ProxyEvent(
        profileId: 'p1',
        kind: rust_api.EventKind.hostKey,
        state: rust_api.ServiceState.awaitingHostKey,
        message: '首次连接需要确认主机密钥',
        fingerprint: 'SHA256:test',
        endpoints: [],
      ),
    );
    await Future<void>.delayed(Duration.zero);
    expect(controller.pendingHostKey?.fingerprint, 'SHA256:test');

    controller.confirmHostKey(true);
    expect(service.confirmedProfileId, 'p1');
    expect(service.confirmedFingerprint, 'SHA256:test');
    expect(service.confirmedAccepted, isTrue);
    expect(controller.pendingHostKey, isNull);

    controller.dispose();
    await service.dispose();
  });

  testWidgets('does not display stored secret values in the editor', (
    tester,
  ) async {
    const profile = rust_api.ProfileView(
      id: 'p1',
      name: '线上 API',
      tags: ['生产'],
      kind: rust_api.ProxyType.http,
      state: rust_api.ServiceState.stopped,
      endpoints: [],
      http: rust_api.HttpView(
        listenPort: 0,
        upstreamUrl: 'https://api.example.com',
        headers: [
          rust_api.SecretFieldView(name: 'Authorization', configured: true),
        ],
        cookies: [],
        useSystemProxy: false,
      ),
    );
    await tester.pumpWidget(
      const MaterialApp(home: ProfileEditorDialog(profile: profile)),
    );

    expect(find.text('Authorization'), findsOneWidget);
    expect(find.text('地址标签（可选）'), findsOneWidget);
    expect(find.text('生产'), findsOneWidget);
    expect(find.text('留空保留现有值'), findsOneWidget);
    expect(find.text('not-displayed-value'), findsNothing);
  });

  testWidgets('validates the profile name before saving', (tester) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: ProfileEditorDialog(initialType: rust_api.ProxyType.http),
      ),
    );

    await tester.tap(find.text('保存'));
    await tester.pump();

    expect(find.text('名称不能为空'), findsOneWidget);
  });

  test('parses browser-copied header pairs and colon format', () {
    final parsed = parseHeaderClipboard('''
sec-ch-ua-platform
"macOS"

sec-fetch-dest
script
sec-fetch-mode: no-cors
''');

    expect(parsed.map((header) => header.name).toList(), [
      'sec-ch-ua-platform',
      'sec-fetch-dest',
      'sec-fetch-mode',
    ]);
    expect(parsed.map((header) => header.value).toList(), [
      '"macOS"',
      'script',
      'no-cors',
    ]);
  });

  test('rejects incomplete or duplicate header imports', () {
    expect(
      () => parseHeaderClipboard('x-request-id'),
      throwsA(isA<FormatException>()),
    );
    expect(
      () => parseHeaderClipboard('x-test: one\nX-Test: two'),
      throwsA(isA<FormatException>()),
    );
  });
}
