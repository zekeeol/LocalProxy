import 'package:flutter_test/flutter_test.dart';
import 'package:local_proxy/main.dart';
import 'package:local_proxy/src/proxy_service.dart';
import 'package:local_proxy/src/rust/api/local_proxy.dart' as rust_api;

class _FakeProxyService implements ProxyService {
  const _FakeProxyService();

  @override
  void confirmHostKey(String id, String fingerprint, bool accepted) {}

  @override
  void deleteProfile(String id) {}

  @override
  Stream<rust_api.ProxyEvent> events() => const Stream.empty();

  @override
  rust_api.AppSettingsView getSettings() => const rust_api.AppSettingsView();

  @override
  rust_api.ProfileView getProfile(String id) =>
      throw UnimplementedError('no profiles in this test');

  @override
  List<rust_api.ProfileSummary> listProfiles() => const [];

  @override
  rust_api.ProfileView saveProfile(rust_api.ProfileDraft draft) =>
      throw UnimplementedError();

  @override
  rust_api.AppSettingsView saveSettings(rust_api.AppSettingsDraft settings) =>
      const rust_api.AppSettingsView();

  @override
  rust_api.StartOutcome startProfile(String id) => throw UnimplementedError();

  @override
  void stopProfile(String id) {}
}

void main() {
  testWidgets('shows the empty proxy workspace', (tester) async {
    await tester.pumpWidget(const MyApp(service: _FakeProxyService()));
    await tester.pump();

    expect(find.text('LocalProxy'), findsOneWidget);
    expect(find.text('还没有代理配置'), findsOneWidget);
    expect(find.text('创建 HTTP 代理'), findsOneWidget);
    expect(find.text('创建 SSH 隧道'), findsOneWidget);
  });

  testWidgets('opens application proxy settings', (tester) async {
    await tester.pumpWidget(const MyApp(service: _FakeProxyService()));
    await tester.pump();

    await tester.tap(find.byTooltip('应用设置'));
    await tester.pumpAndSettle();

    expect(find.text('应用设置'), findsOneWidget);
    expect(find.text('HTTP 代理'), findsOneWidget);
    expect(find.text('SOCKS5 代理'), findsOneWidget);
  });
}
