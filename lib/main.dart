import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'src/profile_controller.dart';
import 'src/proxy_service.dart';
import 'src/rust/api/local_proxy.dart' as rust_api;

const _brandNavy = Color(0xFF0B1F3A);
const _brandTeal = Color(0xFF0B8793);
const _pageBackground = Color(0xFFF3F6FA);
const _sidebarBackground = Color(0xFFEAF2F5);
const _borderColor = Color(0xFFDCE6EB);

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await initializeRust();
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key, this.service});

  final ProxyService? service;

  @override
  Widget build(BuildContext context) {
    final colorScheme = ColorScheme.fromSeed(
      seedColor: _brandTeal,
      brightness: Brightness.light,
    );
    return MaterialApp(
      title: 'LocalProxy',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorScheme: colorScheme,
        useMaterial3: true,
        scaffoldBackgroundColor: _pageBackground,
        appBarTheme: const AppBarTheme(
          backgroundColor: Colors.white,
          foregroundColor: _brandNavy,
          elevation: 0,
          scrolledUnderElevation: 0,
          surfaceTintColor: Colors.transparent,
          toolbarHeight: 70,
          titleTextStyle: TextStyle(
            color: _brandNavy,
            fontSize: 20,
            fontWeight: FontWeight.w800,
            letterSpacing: -.2,
          ),
        ),
        dividerTheme: const DividerThemeData(
          color: _borderColor,
          thickness: 1,
          space: 1,
        ),
        inputDecorationTheme: const InputDecorationTheme(
          border: OutlineInputBorder(),
          isDense: true,
          filled: true,
          fillColor: Colors.white,
          contentPadding: EdgeInsets.symmetric(horizontal: 13, vertical: 13),
          enabledBorder: OutlineInputBorder(
            borderSide: BorderSide(color: _borderColor),
            borderRadius: BorderRadius.all(Radius.circular(10)),
          ),
          focusedBorder: OutlineInputBorder(
            borderSide: BorderSide(color: _brandTeal, width: 1.5),
            borderRadius: BorderRadius.all(Radius.circular(10)),
          ),
        ),
        cardTheme: const CardThemeData(
          color: Colors.white,
          elevation: 0,
          margin: EdgeInsets.zero,
          surfaceTintColor: Colors.transparent,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.all(Radius.circular(14)),
            side: BorderSide(color: _borderColor),
          ),
        ),
        listTileTheme: const ListTileThemeData(
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.all(Radius.circular(12)),
          ),
        ),
      ),
      home: ProxyHomePage(service: service),
    );
  }
}

class ProxyHomePage extends StatefulWidget {
  const ProxyHomePage({super.key, this.service});

  final ProxyService? service;

  @override
  State<ProxyHomePage> createState() => _ProxyHomePageState();
}

class _ProxyHomePageState extends State<ProxyHomePage> {
  late final ProfileController controller;
  String? _shownHostKey;

  @override
  void initState() {
    super.initState();
    controller = ProfileController(service: widget.service);
    controller.initialize();
  }

  @override
  void dispose() {
    controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: controller,
      builder: (context, _) {
        _scheduleHostKeyDialog(context);
        return Scaffold(
          appBar: AppBar(
            titleSpacing: 26,
            title: const Row(
              children: [
                _BrandMark(size: 34),
                SizedBox(width: 11),
                Text('LocalProxy'),
              ],
            ),
            actions: [
              const _LoopbackBadge(),
              const SizedBox(width: 8),
              IconButton(
                tooltip: '应用设置',
                onPressed: controller.isLoading ? null : _openSettings,
                icon: const Icon(Icons.tune_outlined),
              ),
              IconButton(
                tooltip: '刷新',
                onPressed: controller.isLoading ? null : controller.refresh,
                icon: const Icon(Icons.refresh),
              ),
              const SizedBox(width: 18),
            ],
          ),
          body: Row(
            children: [
              SizedBox(width: 304, child: _buildProfilePane(context)),
              const VerticalDivider(width: 1),
              Expanded(child: _buildDetailPane(context)),
            ],
          ),
        );
      },
    );
  }

  Widget _buildProfilePane(BuildContext context) {
    return ColoredBox(
      color: _sidebarBackground,
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 19, 14, 16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                const Expanded(
                  child: Text(
                    '代理配置',
                    style: TextStyle(
                      color: _brandNavy,
                      fontSize: 18,
                      fontWeight: FontWeight.w800,
                    ),
                  ),
                ),
                PopupMenuButton<rust_api.ProxyType>(
                  tooltip: '新建代理',
                  icon: const Icon(Icons.add_circle_outline),
                  onSelected: (type) => _openEditor(type: type),
                  itemBuilder: (context) => const [
                    PopupMenuItem(
                      value: rust_api.ProxyType.http,
                      child: Text('HTTP 反向代理'),
                    ),
                    PopupMenuItem(
                      value: rust_api.ProxyType.ssh,
                      child: Text('SSH 隧道'),
                    ),
                  ],
                ),
              ],
            ),
            const SizedBox(height: 4),
            const Row(
              children: [
                Icon(Icons.lock_outline, size: 14, color: _brandTeal),
                SizedBox(width: 5),
                Text(
                  '127.0.0.1 · 仅本机可访问',
                  style: TextStyle(color: Color(0xFF5B7180), fontSize: 12),
                ),
              ],
            ),
            const SizedBox(height: 14),
            _GlobalProxySummary(settings: controller.settings),
            const SizedBox(height: 16),
            Expanded(
              child: controller.profiles.isEmpty
                  ? _EmptyProfiles(onCreate: () => _openEditor())
                  : ListView.separated(
                      itemCount: controller.profiles.length,
                      separatorBuilder: (_, _) => const SizedBox(height: 8),
                      itemBuilder: (context, index) => _ProfileListTile(
                        profile: controller.profiles[index],
                        selected:
                            controller.selectedId ==
                            controller.profiles[index].id,
                        onTap: () =>
                            controller.select(controller.profiles[index].id),
                      ),
                    ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildDetailPane(BuildContext context) {
    final profile = controller.selected;
    if (profile == null) {
      return _WelcomePane(
        onCreateHttp: () => _openEditor(type: rust_api.ProxyType.http),
        onCreateSsh: () => _openEditor(type: rust_api.ProxyType.ssh),
      );
    }
    final active = _isActive(profile.state);
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(30, 26, 34, 34),
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 940),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Card(
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(20, 18, 14, 17),
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              profile.name,
                              style: Theme.of(context).textTheme.headlineSmall
                                  ?.copyWith(fontWeight: FontWeight.w800),
                            ),
                            const SizedBox(height: 7),
                            Row(
                              children: [
                                _StatusPill(state: profile.state),
                                const SizedBox(width: 10),
                                Text(
                                  _typeLabel(profile.kind),
                                  style: const TextStyle(
                                    color: Color(0xFF667783),
                                    fontSize: 13,
                                  ),
                                ),
                              ],
                            ),
                            if (profile.tags.isNotEmpty) ...[
                              const SizedBox(height: 11),
                              _TagWrap(tags: profile.tags),
                            ],
                          ],
                        ),
                      ),
                      OutlinedButton.icon(
                        onPressed: active
                            ? () => controller.stop(profile.id)
                            : profile.state == rust_api.ServiceState.starting ||
                                  profile.state ==
                                      rust_api.ServiceState.awaitingHostKey
                            ? null
                            : () => controller.start(profile.id),
                        icon: Icon(active ? Icons.stop : Icons.play_arrow),
                        label: Text(active ? '停止' : '启动'),
                      ),
                      const SizedBox(width: 4),
                      IconButton(
                        tooltip: '编辑',
                        onPressed: active
                            ? null
                            : () => _openEditor(profile: profile),
                        icon: const Icon(Icons.edit_outlined),
                      ),
                      PopupMenuButton<String>(
                        onSelected: (value) {
                          if (value == 'delete') _deleteProfile(profile);
                        },
                        itemBuilder: (context) => const [
                          PopupMenuItem(value: 'delete', child: Text('删除配置')),
                        ],
                      ),
                    ],
                  ),
                ),
              ),
              if (profile.error != null &&
                  profile.state == rust_api.ServiceState.error)
                Padding(
                  padding: const EdgeInsets.only(top: 18),
                  child: _ErrorBanner(message: profile.error!),
                ),
              const SizedBox(height: 24),
              _SectionCard(
                title: '本地端点',
                icon: Icons.router_outlined,
                child: _LocalEndpointsPanel(
                  profile: profile,
                  settings: controller.settings,
                ),
              ),
              const SizedBox(height: 16),
              _buildConfigurationCard(context, profile),
              const SizedBox(height: 16),
              _SectionCard(
                title: '运行日志',
                icon: Icons.subject_outlined,
                child: controller.logs.isEmpty
                    ? Text(
                        '暂无运行日志',
                        style: TextStyle(color: Colors.grey.shade600),
                      )
                    : Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: controller.logs
                            .take(12)
                            .map(
                              (message) => Padding(
                                padding: const EdgeInsets.only(bottom: 7),
                                child: Text(
                                  message,
                                  style: TextStyle(
                                    color: Colors.grey.shade700,
                                    fontSize: 13,
                                  ),
                                ),
                              ),
                            )
                            .toList(),
                      ),
              ),
              if (controller.errorMessage != null)
                Padding(
                  padding: const EdgeInsets.only(top: 16),
                  child: _ErrorBanner(message: controller.errorMessage!),
                ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildConfigurationCard(
    BuildContext context,
    rust_api.ProfileView profile,
  ) {
    if (profile.http != null) {
      final http = profile.http!;
      return _SectionCard(
        title: 'HTTP 配置',
        icon: Icons.http_outlined,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            _InfoLine(label: '上游地址', value: http.upstreamUrl),
            _InfoLine(
              label: '地址标签',
              value: profile.tags.isEmpty ? '未添加' : profile.tags.join(' · '),
            ),
            _InfoLine(
              label: '监听端口',
              value: http.listenPort == 0 ? '随机端口' : '${http.listenPort}',
            ),
            _InfoLine(
              label: '鉴权 headers',
              value: http.headers.isEmpty
                  ? '未配置'
                  : '${http.headers.length} 项（值已隐藏）',
            ),
            _InfoLine(
              label: '鉴权 cookies',
              value: http.cookies.isEmpty
                  ? '未配置'
                  : '${http.cookies.length} 项（值已隐藏）',
            ),
            _InfoLine(
              label: '系统 HTTP 代理',
              value: http.useSystemProxy
                  ? _proxyDisplay(controller.settings?.httpProxy)
                  : '未启用',
            ),
          ],
        ),
      );
    }
    final ssh = profile.ssh!;
    return _SectionCard(
      title: 'SSH 配置',
      icon: Icons.terminal_outlined,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _InfoLine(
            label: '远端',
            value: '${ssh.username}@${ssh.host}:${ssh.port}',
          ),
          _InfoLine(
            label: '地址标签',
            value: profile.tags.isEmpty ? '未添加' : profile.tags.join(' · '),
          ),
          _InfoLine(
            label: '认证',
            value: ssh.authMode == rust_api.SshAuthMode.password
                ? '密码（已保存）'
                : '私钥 ${ssh.privateKeyPath}',
          ),
          _InfoLine(
            label: '本地转发',
            value: ssh.forwards.isEmpty ? '未配置' : '${ssh.forwards.length} 条',
          ),
          _InfoLine(
            label: '系统 SOCKS5 代理',
            value: ssh.useSystemProxy
                ? _proxyDisplay(controller.settings?.socksProxy)
                : '未启用（SSH 直连）',
          ),
        ],
      ),
    );
  }

  Future<void> _openEditor({
    rust_api.ProfileView? profile,
    rust_api.ProxyType? type,
  }) async {
    final draft = await showDialog<rust_api.ProfileDraft>(
      context: context,
      builder: (context) =>
          ProfileEditorDialog(profile: profile, initialType: type),
    );
    if (draft != null && mounted) controller.save(draft);
  }

  Future<void> _openSettings() async {
    final settings = controller.settings ?? const rust_api.AppSettingsView();
    final draft = await showDialog<rust_api.AppSettingsDraft>(
      context: context,
      builder: (context) => AppSettingsDialog(settings: settings),
    );
    if (draft != null && mounted) {
      controller.saveSettings(draft);
    }
  }

  Future<void> _deleteProfile(rust_api.ProfileView profile) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('删除代理配置？'),
        content: Text('“${profile.name}” 的配置和 Keychain 密钥将被删除。'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('取消'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('删除'),
          ),
        ],
      ),
    );
    if (confirmed == true && mounted) controller.remove(profile.id);
  }

  void _scheduleHostKeyDialog(BuildContext context) {
    final pending = controller.pendingHostKey;
    final pendingKey = pending == null
        ? null
        : '${pending.profileId}:${pending.fingerprint}';
    if (pending == null || _shownHostKey == pendingKey) return;
    _shownHostKey = pendingKey;
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      if (!mounted) return;
      final accepted = await showDialog<bool>(
        context: context,
        barrierDismissible: false,
        builder: (context) => AlertDialog(
          title: const Text('确认 SSH 主机密钥'),
          content: SizedBox(
            width: 520,
            child: SelectableText(
              '${pending.message}\n\nSHA-256 指纹：\n${pending.fingerprint}\n\n只在确认远端主机可信时继续。',
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('拒绝'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(context, true),
              child: const Text('信任并继续'),
            ),
          ],
        ),
      );
      if (!mounted) return;
      controller.confirmHostKey(accepted == true);
      _shownHostKey = null;
    });
  }
}

class _BrandMark extends StatelessWidget {
  const _BrandMark({required this.size});

  final double size;

  @override
  Widget build(BuildContext context) {
    return ClipRRect(
      borderRadius: BorderRadius.circular(size * .22),
      child: Image.asset(
        'assets/brand/localproxy-icon.png',
        width: size,
        height: size,
        fit: BoxFit.cover,
        filterQuality: FilterQuality.medium,
        errorBuilder: (context, error, stackTrace) => Container(
          width: size,
          height: size,
          color: _brandNavy,
          alignment: Alignment.center,
          child: Icon(
            Icons.hub_outlined,
            size: size * .64,
            color: Colors.white,
          ),
        ),
      ),
    );
  }
}

class _LoopbackBadge extends StatelessWidget {
  const _LoopbackBadge();

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
      decoration: BoxDecoration(
        color: _brandTeal.withValues(alpha: .09),
        borderRadius: BorderRadius.circular(99),
        border: Border.all(color: _brandTeal.withValues(alpha: .18)),
      ),
      child: const Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.lock_outline, size: 14, color: _brandTeal),
          SizedBox(width: 5),
          Text(
            '仅回环地址',
            style: TextStyle(
              color: _brandTeal,
              fontSize: 12,
              fontWeight: FontWeight.w700,
            ),
          ),
        ],
      ),
    );
  }
}

class _GlobalProxySummary extends StatelessWidget {
  const _GlobalProxySummary({required this.settings});

  final rust_api.AppSettingsView? settings;

  @override
  Widget build(BuildContext context) {
    final http = settings?.httpProxy;
    final socks = settings?.socksProxy;
    return Container(
      padding: const EdgeInsets.fromLTRB(11, 10, 11, 10),
      decoration: BoxDecoration(
        color: Colors.white.withValues(alpha: .72),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: Colors.white.withValues(alpha: .9)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Row(
            children: [
              Icon(Icons.alt_route_outlined, size: 15, color: _brandTeal),
              SizedBox(width: 6),
              Text(
                '应用级出站代理',
                style: TextStyle(
                  color: _brandNavy,
                  fontSize: 12,
                  fontWeight: FontWeight.w800,
                ),
              ),
            ],
          ),
          const SizedBox(height: 7),
          _ProxySummaryLine(label: 'HTTP', endpoint: http),
          const SizedBox(height: 3),
          _ProxySummaryLine(label: 'SOCKS5', endpoint: socks),
        ],
      ),
    );
  }
}

class _ProxySummaryLine extends StatelessWidget {
  const _ProxySummaryLine({required this.label, required this.endpoint});

  final String label;
  final rust_api.ProxyEndpoint? endpoint;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        SizedBox(
          width: 48,
          child: Text(
            label,
            style: const TextStyle(
              color: Color(0xFF60727D),
              fontSize: 11,
              fontWeight: FontWeight.w700,
            ),
          ),
        ),
        Expanded(
          child: Text(
            endpoint == null ? '未配置' : _proxyAddress(endpoint!),
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(
              color: endpoint == null ? const Color(0xFF8A989F) : _brandNavy,
              fontSize: 11,
              fontFamily: 'SF Mono',
            ),
          ),
        ),
      ],
    );
  }
}

class _TagWrap extends StatelessWidget {
  const _TagWrap({required this.tags, this.compact = false});

  final List<String> tags;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    return Wrap(
      spacing: compact ? 4 : 6,
      runSpacing: compact ? 4 : 6,
      children: tags
          .map(
            (tag) => Container(
              padding: EdgeInsets.symmetric(
                horizontal: compact ? 7 : 9,
                vertical: compact ? 3 : 4,
              ),
              decoration: BoxDecoration(
                color: _brandTeal.withValues(alpha: compact ? .09 : .12),
                borderRadius: BorderRadius.circular(99),
              ),
              child: Text(
                tag,
                style: TextStyle(
                  color: _brandTeal,
                  fontSize: compact ? 10 : 11,
                  fontWeight: FontWeight.w700,
                ),
              ),
            ),
          )
          .toList(),
    );
  }
}

class _LocalEndpointsPanel extends StatelessWidget {
  const _LocalEndpointsPanel({required this.profile, this.settings});

  final rust_api.ProfileView profile;
  final rust_api.AppSettingsView? settings;

  @override
  Widget build(BuildContext context) {
    final live = profile.endpoints.isNotEmpty;
    final endpoints = live ? profile.endpoints : _configuredEndpoints(profile);
    final sshUsesSystemProxy = profile.ssh?.useSystemProxy == true;
    final socksProxy = settings?.socksProxy;
    if (endpoints.isEmpty) {
      return Row(
        children: [
          Icon(Icons.info_outline, size: 18, color: Colors.grey.shade500),
          const SizedBox(width: 8),
          Text('当前配置没有本地监听端点', style: TextStyle(color: Colors.grey.shade600)),
        ],
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Icon(
              live ? Icons.check_circle_outline : Icons.visibility_outlined,
              size: 17,
              color: live ? Colors.green.shade700 : const Color(0xFF7A8B95),
            ),
            const SizedBox(width: 7),
            Text(
              live ? '实际运行地址' : '本地地址预览',
              style: const TextStyle(
                color: Color(0xFF52636D),
                fontSize: 12,
                fontWeight: FontWeight.w700,
              ),
            ),
            const Spacer(),
            Text(
              live ? '可复制使用' : '启动后生效',
              style: TextStyle(color: Colors.grey.shade500, fontSize: 12),
            ),
          ],
        ),
        const SizedBox(height: 12),
        if (sshUsesSystemProxy) ...[
          Container(
            width: double.infinity,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            decoration: BoxDecoration(
              color: const Color(0xFFFFF8EA),
              borderRadius: BorderRadius.circular(10),
              border: Border.all(color: const Color(0xFFF4D79B)),
            ),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Icon(
                  Icons.alt_route_outlined,
                  size: 16,
                  color: Color(0xFFAD7417),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    'SSH 会话出站：LocalProxy → '
                    '${socksProxy == null ? '应用 SOCKS5' : _proxyAddress(socksProxy)}'
                    ' → SSH → 目标地址',
                    style: const TextStyle(
                      color: Color(0xFF8D641F),
                      fontSize: 11,
                      height: 1.35,
                    ),
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
        ],
        Wrap(
          spacing: 12,
          runSpacing: 12,
          children: endpoints
              .asMap()
              .entries
              .map(
                (entry) => _EndpointCard(
                  endpoint: entry.value,
                  tags: profile.tags,
                  endpointTag: _endpointTag(profile, endpoints, entry.key),
                  live: live,
                ),
              )
              .toList(),
        ),
      ],
    );
  }
}

class _ProfileListTile extends StatelessWidget {
  const _ProfileListTile({
    required this.profile,
    required this.selected,
    required this.onTap,
  });

  final rust_api.ProfileSummary profile;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final background = selected
        ? _brandTeal.withValues(alpha: .14)
        : Colors.transparent;
    return Material(
      color: background,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(13),
        side: selected
            ? BorderSide(color: _brandTeal.withValues(alpha: .20))
            : BorderSide.none,
      ),
      borderRadius: BorderRadius.circular(12),
      child: InkWell(
        borderRadius: BorderRadius.circular(12),
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(12, 11, 10, 11),
          child: Row(
            children: [
              Container(
                width: 35,
                height: 35,
                decoration: BoxDecoration(
                  color: selected
                      ? Colors.white.withValues(alpha: .72)
                      : Colors.white.withValues(alpha: .76),
                  borderRadius: BorderRadius.circular(10),
                ),
                alignment: Alignment.center,
                child: Icon(
                  profile.kind == rust_api.ProxyType.http
                      ? Icons.http_outlined
                      : Icons.terminal_outlined,
                  size: 20,
                  color: _brandNavy,
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      profile.name,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontWeight: FontWeight.w600),
                    ),
                    const SizedBox(height: 4),
                    Text(
                      profile.endpoints.isEmpty
                          ? _stateLabel(profile.state)
                          : profile.endpoints.map(_endpointAddress).join(' · '),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(
                        color: Color(0xFF60727D),
                        fontSize: 11,
                        fontFamily: 'SF Mono',
                      ),
                    ),
                    if (profile.tags.isNotEmpty) ...[
                      const SizedBox(height: 6),
                      _TagWrap(
                        tags: profile.tags.take(2).toList(),
                        compact: true,
                      ),
                    ],
                  ],
                ),
              ),
              _StatusDot(state: profile.state),
            ],
          ),
        ),
      ),
    );
  }
}

class _EmptyProfiles extends StatelessWidget {
  const _EmptyProfiles({required this.onCreate});

  final VoidCallback onCreate;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        Icon(
          Icons.layers_clear_outlined,
          size: 38,
          color: Colors.grey.shade400,
        ),
        const SizedBox(height: 12),
        Text(
          '还没有代理配置',
          style: TextStyle(
            color: Colors.grey.shade700,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: 6),
        Text(
          '创建一个本地端点开始使用',
          textAlign: TextAlign.center,
          style: TextStyle(color: Colors.grey.shade500, fontSize: 12),
        ),
        const SizedBox(height: 16),
        OutlinedButton.icon(
          onPressed: onCreate,
          icon: const Icon(Icons.add, size: 18),
          label: const Text('新建代理'),
        ),
      ],
    );
  }
}

class _WelcomePane extends StatelessWidget {
  const _WelcomePane({required this.onCreateHttp, required this.onCreateSsh});

  final VoidCallback onCreateHttp;
  final VoidCallback onCreateSsh;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 620),
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              const _BrandMark(size: 86),
              const SizedBox(height: 22),
              Text(
                '把需要的服务接到本机',
                style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                  fontWeight: FontWeight.w700,
                ),
              ),
              const SizedBox(height: 10),
              Text(
                'LocalProxy 在本机回环地址提供 HTTP 反向代理和 SSH 本地端口转发，SSH 会话可选经过本机 SOCKS5。',
                textAlign: TextAlign.center,
                style: const TextStyle(color: Color(0xFF667783), height: 1.5),
              ),
              const SizedBox(height: 28),
              Wrap(
                spacing: 12,
                children: [
                  FilledButton.icon(
                    onPressed: onCreateHttp,
                    icon: const Icon(Icons.http_outlined),
                    label: const Text('创建 HTTP 代理'),
                  ),
                  OutlinedButton.icon(
                    onPressed: onCreateSsh,
                    icon: const Icon(Icons.terminal_outlined),
                    label: const Text('创建 SSH 隧道'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _SectionCard extends StatelessWidget {
  const _SectionCard({
    required this.title,
    required this.icon,
    required this.child,
  });

  final String title;
  final IconData icon;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(21, 18, 21, 20),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Container(
                  width: 30,
                  height: 30,
                  decoration: BoxDecoration(
                    color: _brandTeal.withValues(alpha: .10),
                    borderRadius: BorderRadius.circular(9),
                  ),
                  alignment: Alignment.center,
                  child: Icon(icon, size: 18, color: _brandTeal),
                ),
                const SizedBox(width: 9),
                Text(
                  title,
                  style: const TextStyle(
                    color: _brandNavy,
                    fontWeight: FontWeight.w800,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 15),
            child,
          ],
        ),
      ),
    );
  }
}

class _EndpointCard extends StatelessWidget {
  const _EndpointCard({
    required this.endpoint,
    required this.tags,
    required this.endpointTag,
    required this.live,
  });

  final rust_api.Endpoint endpoint;
  final List<String> tags;
  final String? endpointTag;
  final bool live;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 332,
      padding: const EdgeInsets.fromLTRB(15, 14, 12, 13),
      decoration: BoxDecoration(
        color: live ? const Color(0xFFF0F8F7) : const Color(0xFFF7F9FA),
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: live ? _brandTeal.withValues(alpha: .18) : _borderColor,
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                width: 32,
                height: 32,
                decoration: BoxDecoration(
                  color: _brandTeal.withValues(alpha: .12),
                  borderRadius: BorderRadius.circular(9),
                ),
                alignment: Alignment.center,
                child: Icon(
                  _endpointIcon(endpoint.kind),
                  size: 19,
                  color: _brandTeal,
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      _endpointKindLabel(endpoint.kind),
                      style: const TextStyle(
                        color: _brandNavy,
                        fontWeight: FontWeight.w800,
                      ),
                    ),
                    const SizedBox(height: 3),
                    Text(
                      live ? '运行中' : '配置预览',
                      style: TextStyle(
                        color: live
                            ? Colors.green.shade700
                            : const Color(0xFF7A8B95),
                        fontSize: 12,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ],
                ),
              ),
              IconButton(
                tooltip: endpoint.port == 0 ? '启动后才能复制地址' : '复制地址',
                visualDensity: VisualDensity.compact,
                onPressed: endpoint.port == 0
                    ? null
                    : () => _copyEndpoint(context),
                icon: const Icon(Icons.copy_outlined, size: 17),
              ),
            ],
          ),
          const SizedBox(height: 11),
          SelectableText(
            _endpointAddress(endpoint),
            style: const TextStyle(
              color: _brandNavy,
              fontSize: 13,
              fontWeight: FontWeight.w700,
              fontFamily: 'SF Mono',
            ),
          ),
          if (endpoint.description.isNotEmpty) ...[
            const SizedBox(height: 5),
            SelectableText(
              _endpointDetail(endpoint),
              style: const TextStyle(color: Color(0xFF6C7D86), fontSize: 11),
            ),
          ],
          if (tags.isNotEmpty || endpointTag != null) ...[
            const SizedBox(height: 9),
            _TagWrap(tags: _mergeTags(tags, endpointTag), compact: true),
          ],
        ],
      ),
    );
  }

  void _copyEndpoint(BuildContext context) {
    final text = _endpointAddress(endpoint);
    Clipboard.setData(ClipboardData(text: text));
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('地址已复制'), duration: Duration(seconds: 1)),
    );
  }
}

class _InfoLine extends StatelessWidget {
  const _InfoLine({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 100,
            child: Text(label, style: TextStyle(color: Colors.grey.shade600)),
          ),
          Expanded(child: SelectableText(value)),
        ],
      ),
    );
  }
}

class _ErrorBanner extends StatelessWidget {
  const _ErrorBanner({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(13),
      decoration: BoxDecoration(
        color: Colors.red.shade50,
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: Colors.red.shade100),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Icon(Icons.error_outline, color: Colors.red.shade700, size: 20),
          const SizedBox(width: 9),
          Expanded(
            child: Text(message, style: TextStyle(color: Colors.red.shade900)),
          ),
        ],
      ),
    );
  }
}

class _StatusPill extends StatelessWidget {
  const _StatusPill({required this.state});

  final rust_api.ServiceState state;

  @override
  Widget build(BuildContext context) {
    final color = _stateColor(state);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 4),
      decoration: BoxDecoration(
        color: color.withValues(alpha: .12),
        borderRadius: BorderRadius.circular(99),
      ),
      child: Text(
        _stateLabel(state),
        style: TextStyle(
          color: color,
          fontSize: 12,
          fontWeight: FontWeight.w700,
        ),
      ),
    );
  }
}

class _StatusDot extends StatelessWidget {
  const _StatusDot({required this.state});

  final rust_api.ServiceState state;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 9,
      height: 9,
      decoration: BoxDecoration(
        color: _stateColor(state),
        shape: BoxShape.circle,
      ),
    );
  }
}

class ProfileEditorDialog extends StatefulWidget {
  const ProfileEditorDialog({super.key, this.profile, this.initialType});

  final rust_api.ProfileView? profile;
  final rust_api.ProxyType? initialType;

  @override
  State<ProfileEditorDialog> createState() => _ProfileEditorDialogState();
}

class _ProfileEditorDialogState extends State<ProfileEditorDialog> {
  late rust_api.ProxyType _type;
  late final TextEditingController _name;
  late final TextEditingController _tags;
  late final TextEditingController _upstream;
  late final TextEditingController _httpPort;
  late final TextEditingController _sshHost;
  late final TextEditingController _sshPort;
  late final TextEditingController _sshUsername;
  late final TextEditingController _privateKeyPath;
  late final TextEditingController _password;
  late final TextEditingController _passphrase;
  late bool _passwordConfigured;
  late bool _passphraseConfigured;
  late bool _httpUseSystemProxy;
  late bool _sshUseSystemProxy;
  late rust_api.SshAuthMode _authMode;
  final List<_SecretRow> _headers = [];
  final List<_SecretRow> _cookies = [];
  final List<_ForwardRow> _forwards = [];
  final List<String> _removedHeaders = [];
  final List<String> _removedCookies = [];
  String? _validationError;

  @override
  void initState() {
    super.initState();
    final profile = widget.profile;
    _type = profile?.kind ?? widget.initialType ?? rust_api.ProxyType.http;
    _name = TextEditingController(text: profile?.name ?? '');
    _tags = TextEditingController(text: profile?.tags.join('，') ?? '');
    final http = profile?.http;
    _httpUseSystemProxy = http?.useSystemProxy ?? false;
    _upstream = TextEditingController(text: http?.upstreamUrl ?? 'https://');
    _httpPort = TextEditingController(
      text: http == null || http.listenPort == 0 ? '' : '${http.listenPort}',
    );
    _headers.addAll(
      http?.headers.map(
            (field) => _SecretRow(
              name: field.name,
              originalName: field.name,
              existing: true,
            ),
          ) ??
          const <_SecretRow>[],
    );
    _cookies.addAll(
      http?.cookies.map(
            (field) => _SecretRow(
              name: field.name,
              originalName: field.name,
              existing: true,
            ),
          ) ??
          const <_SecretRow>[],
    );
    final ssh = profile?.ssh;
    _sshHost = TextEditingController(text: ssh?.host ?? '');
    _sshPort = TextEditingController(text: ssh == null ? '22' : '${ssh.port}');
    _sshUsername = TextEditingController(text: ssh?.username ?? '');
    _privateKeyPath = TextEditingController(text: ssh?.privateKeyPath ?? '');
    _password = TextEditingController();
    _passphrase = TextEditingController();
    _passwordConfigured = ssh?.passwordConfigured ?? false;
    _passphraseConfigured = ssh?.passphraseConfigured ?? false;
    _authMode = ssh?.authMode ?? rust_api.SshAuthMode.password;
    _sshUseSystemProxy = ssh?.useSystemProxy ?? false;
    _forwards.addAll(
      ssh?.forwards.map(
            (forward) => _ForwardRow(
              localPort: forward.localPort == 0 ? '' : '${forward.localPort}',
              remoteHost: forward.remoteHost,
              remotePort: '${forward.remotePort}',
              tag: forward.tag,
            ),
          ) ??
          const <_ForwardRow>[],
    );
  }

  @override
  void dispose() {
    _name.dispose();
    _tags.dispose();
    _upstream.dispose();
    _httpPort.dispose();
    _sshHost.dispose();
    _sshPort.dispose();
    _sshUsername.dispose();
    _privateKeyPath.dispose();
    _password.dispose();
    _passphrase.dispose();
    for (final row in [..._headers, ..._cookies]) {
      row.dispose();
    }
    for (final row in _forwards) {
      row.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(widget.profile == null ? '新建代理' : '编辑代理'),
      content: SizedBox(
        width: 680,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              TextFormField(
                controller: _name,
                decoration: const InputDecoration(
                  labelText: '名称',
                  hintText: '例如：线上 API',
                ),
              ),
              const SizedBox(height: 14),
              TextFormField(
                controller: _tags,
                decoration: const InputDecoration(
                  labelText: '地址标签（可选）',
                  hintText: '例如：生产、API、Redis',
                  helperText: '多个标签用逗号分隔，会显示在代理地址卡片和配置列表中',
                  prefixIcon: Icon(Icons.sell_outlined),
                ),
              ),
              const SizedBox(height: 14),
              DropdownButtonFormField<rust_api.ProxyType>(
                initialValue: _type,
                decoration: const InputDecoration(labelText: '代理类型'),
                items: const [
                  DropdownMenuItem(
                    value: rust_api.ProxyType.http,
                    child: Text('HTTP 反向代理'),
                  ),
                  DropdownMenuItem(
                    value: rust_api.ProxyType.ssh,
                    child: Text('SSH 隧道'),
                  ),
                ],
                onChanged: widget.profile == null
                    ? (value) => setState(() => _type = value!)
                    : null,
              ),
              const SizedBox(height: 20),
              if (_type == rust_api.ProxyType.http)
                _buildHttpForm()
              else
                _buildSshForm(),
              if (_validationError != null) ...[
                const SizedBox(height: 14),
                _ErrorBanner(message: _validationError!),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('取消'),
        ),
        FilledButton.icon(
          onPressed: _save,
          icon: const Icon(Icons.save_outlined, size: 18),
          label: const Text('保存'),
        ),
      ],
    );
  }

  Widget _buildHttpForm() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        TextFormField(
          controller: _upstream,
          decoration: const InputDecoration(
            labelText: '上游基础 URL',
            hintText: 'https://api.example.com/v1',
            helperText: '本地请求路径会追加到此地址，query 参数保持不变',
          ),
        ),
        const SizedBox(height: 14),
        _PortField(controller: _httpPort, label: '本地监听端口', hint: '留空使用随机端口'),
        const SizedBox(height: 8),
        SwitchListTile.adaptive(
          contentPadding: EdgeInsets.zero,
          title: const Text('使用应用级 HTTP 代理'),
          subtitle: const Text('访问上游时经过应用设置中配置的 HTTP 代理'),
          value: _httpUseSystemProxy,
          onChanged: (value) => setState(() => _httpUseSystemProxy = value),
        ),
        const SizedBox(height: 20),
        _SecretTable(
          title: '鉴权 Headers',
          hint: '例如 Authorization',
          rows: _headers,
          onAdd: () => setState(() => _headers.add(_SecretRow())),
          onRemove: (row) => _removeSecretRow(_headers, _removedHeaders, row),
          onImport: _importHeaders,
        ),
        const SizedBox(height: 18),
        _SecretTable(
          title: 'Cookies',
          hint: '例如 session',
          rows: _cookies,
          onAdd: () => setState(() => _cookies.add(_SecretRow())),
          onRemove: (row) => _removeSecretRow(_cookies, _removedCookies, row),
        ),
      ],
    );
  }

  Widget _buildSshForm() {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              flex: 3,
              child: TextFormField(
                controller: _sshHost,
                decoration: const InputDecoration(labelText: 'SSH 主机'),
              ),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: TextFormField(
                controller: _sshPort,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(labelText: '端口'),
              ),
            ),
          ],
        ),
        const SizedBox(height: 14),
        TextFormField(
          controller: _sshUsername,
          decoration: const InputDecoration(labelText: '用户名'),
        ),
        const SizedBox(height: 14),
        DropdownButtonFormField<rust_api.SshAuthMode>(
          initialValue: _authMode,
          decoration: const InputDecoration(labelText: '认证方式'),
          items: const [
            DropdownMenuItem(
              value: rust_api.SshAuthMode.password,
              child: Text('密码'),
            ),
            DropdownMenuItem(
              value: rust_api.SshAuthMode.privateKey,
              child: Text('私钥文件'),
            ),
          ],
          onChanged: (value) => setState(() => _authMode = value!),
        ),
        const SizedBox(height: 14),
        if (_authMode == rust_api.SshAuthMode.password)
          TextFormField(
            controller: _password,
            obscureText: true,
            decoration: InputDecoration(
              labelText: '密码',
              hintText: _passwordConfigured ? '留空保留现有密码' : null,
            ),
          )
        else ...[
          TextFormField(
            controller: _privateKeyPath,
            decoration: const InputDecoration(
              labelText: '私钥路径',
              hintText: '/Users/me/.ssh/id_ed25519',
            ),
          ),
          const SizedBox(height: 14),
          TextFormField(
            controller: _passphrase,
            obscureText: true,
            decoration: InputDecoration(
              labelText: '私钥口令（可选）',
              hintText: _passphraseConfigured ? '留空保留现有口令' : null,
            ),
          ),
        ],
        const SizedBox(height: 22),
        SwitchListTile.adaptive(
          contentPadding: EdgeInsets.zero,
          title: const Text('使用应用级 SOCKS5 代理连接 SSH'),
          subtitle: const Text(
            '客户端 → LocalProxy → 应用设置中的 SOCKS5 → SSH 会话 → 目标地址',
          ),
          value: _sshUseSystemProxy,
          onChanged: (value) => setState(() => _sshUseSystemProxy = value),
        ),
        if (_sshUseSystemProxy)
          const Text(
            'LocalProxy 会先连接应用设置中的 SOCKS5 地址，再在这条连接上建立 SSH 会话。',
            style: TextStyle(color: Color(0xFF71828C), fontSize: 11),
          ),
        const SizedBox(height: 14),
        _ForwardTable(
          rows: _forwards,
          onAdd: () => setState(() => _forwards.add(_ForwardRow())),
          onRemove: (row) => setState(() {
            _forwards.remove(row);
            row.dispose();
          }),
        ),
      ],
    );
  }

  Future<void> _importHeaders() async {
    final clipboard = await Clipboard.getData(Clipboard.kTextPlain);
    if (!mounted) return;
    final text = await showDialog<String>(
      context: context,
      builder: (context) =>
          _HeaderImportDialog(initialText: clipboard?.text ?? ''),
    );
    if (!mounted || text == null) return;
    try {
      final parsed = parseHeaderClipboard(text);
      setState(() {
        _validationError = null;
        for (final header in parsed) {
          _SecretRow? existing;
          for (final row in _headers) {
            if (row.nameController.text.trim().toLowerCase() ==
                header.name.toLowerCase()) {
              existing = row;
              break;
            }
          }
          if (existing == null) {
            final row = _SecretRow(name: header.name);
            row.valueController.text = header.value;
            _headers.add(row);
          } else {
            existing.valueController.text = header.value;
          }
        }
      });
    } on FormatException catch (error) {
      setState(() => _validationError = error.message);
    }
  }

  void _removeSecretRow(
    List<_SecretRow> rows,
    List<String> removed,
    _SecretRow row,
  ) {
    setState(() {
      rows.remove(row);
      if (row.existing && row.originalName != null) {
        removed.add(row.originalName!);
      }
      row.dispose();
    });
  }

  void _save() {
    try {
      final name = _name.text.trim();
      if (name.isEmpty) throw const FormatException('名称不能为空');
      final draft = _type == rust_api.ProxyType.http
          ? _buildHttpDraft(name)
          : _buildSshDraft(name);
      Navigator.pop(context, draft);
    } on FormatException catch (error) {
      setState(() => _validationError = error.message);
    }
  }

  rust_api.ProfileDraft _buildHttpDraft(String name) {
    final upstream = _upstream.text.trim();
    if (upstream.isEmpty) throw const FormatException('上游 URL 不能为空');
    final tags = _parseTags(_tags.text);
    final headers = _buildSecretPatches(_headers, _removedHeaders, 'header');
    final cookies = _buildSecretPatches(_cookies, _removedCookies, 'cookie');
    return rust_api.ProfileDraft(
      id: widget.profile?.id,
      name: name,
      tags: tags,
      kind: rust_api.ProxyType.http,
      http: rust_api.HttpDraft(
        listenPort: _parsePort(_httpPort.text, 'HTTP 监听端口'),
        upstreamUrl: upstream,
        headers: headers,
        cookies: cookies,
        useSystemProxy: _httpUseSystemProxy,
      ),
    );
  }

  rust_api.ProfileDraft _buildSshDraft(String name) {
    final host = _sshHost.text.trim();
    final username = _sshUsername.text.trim();
    final tags = _parseTags(_tags.text);
    if (host.isEmpty) throw const FormatException('SSH 主机不能为空');
    if (username.isEmpty) throw const FormatException('用户名不能为空');
    final privateKeyPath = _privateKeyPath.text.trim();
    if (_authMode == rust_api.SshAuthMode.privateKey &&
        privateKeyPath.isEmpty) {
      throw const FormatException('私钥认证需要填写私钥路径');
    }
    final password = _password.text;
    if (_authMode == rust_api.SshAuthMode.password &&
        password.isEmpty &&
        !_passwordConfigured) {
      throw const FormatException('请输入 SSH 密码');
    }
    final forwards = <rust_api.ForwardDraft>[];
    for (final row in _forwards) {
      if (row.remoteHost.text.trim().isEmpty &&
          row.remotePort.text.trim().isEmpty &&
          row.localPort.text.trim().isEmpty &&
          row.tag.text.trim().isEmpty) {
        continue;
      }
      final remoteHost = row.remoteHost.text.trim();
      if (remoteHost.isEmpty) throw const FormatException('本地转发远端主机不能为空');
      final tag = row.tag.text.trim();
      if (tag.runes.length > 32) {
        throw const FormatException('单个转发标签不能超过 32 个字符');
      }
      forwards.add(
        rust_api.ForwardDraft(
          localPort: _parsePort(row.localPort.text, '本地转发端口'),
          remoteHost: remoteHost,
          remotePort: _parsePort(row.remotePort.text, '远端端口', allowZero: false),
          tag: tag,
        ),
      );
    }
    return rust_api.ProfileDraft(
      id: widget.profile?.id,
      name: name,
      tags: tags,
      kind: rust_api.ProxyType.ssh,
      ssh: rust_api.SshDraft(
        host: host,
        port: _parsePort(_sshPort.text, 'SSH 端口', allowZero: false),
        username: username,
        authMode: _authMode,
        privateKeyPath: privateKeyPath,
        password: _secretPatch(
          name: 'password',
          value: password,
          configured: _passwordConfigured,
        ),
        passphrase: _secretPatch(
          name: 'passphrase',
          value: _passphrase.text,
          configured: _passphraseConfigured,
        ),
        forwards: forwards,
        useSystemProxy: _sshUseSystemProxy,
      ),
    );
  }

  List<rust_api.SecretPatch> _buildSecretPatches(
    List<_SecretRow> rows,
    List<String> removed,
    String label,
  ) {
    final patches = <rust_api.SecretPatch>[];
    final names = <String>{};
    String normalizedName(String name) =>
        label == 'header' ? name.toLowerCase() : name;
    for (final row in rows) {
      final name = row.nameController.text.trim();
      final value = row.valueController.text;
      if (name.isEmpty && value.isEmpty) continue;
      if (name.isEmpty) throw FormatException('$label 名称不能为空');
      if (!names.add(normalizedName(name))) {
        throw FormatException('$label 名称不能重复: $name');
      }
      final originalName = row.originalName?.trim();
      final renamed =
          originalName != null &&
          normalizedName(originalName) != normalizedName(name);
      if (renamed && value.isEmpty) {
        throw FormatException('$name 已改名，请重新填写值');
      }
      if (renamed) {
        patches.add(
          rust_api.SecretPatch(
            name: originalName,
            kind: rust_api.SecretPatchKind.clear,
          ),
        );
      }
      if (value.isEmpty) {
        if (!row.existing) throw FormatException('$name 的值不能为空');
        patches.add(
          rust_api.SecretPatch(name: name, kind: rust_api.SecretPatchKind.keep),
        );
      } else {
        patches.add(
          rust_api.SecretPatch(
            name: name,
            value: value,
            kind: rust_api.SecretPatchKind.set_,
          ),
        );
      }
    }
    for (final name in removed) {
      if (names.add(normalizedName(name))) {
        patches.add(
          rust_api.SecretPatch(
            name: name,
            kind: rust_api.SecretPatchKind.clear,
          ),
        );
      }
    }
    return patches;
  }

  rust_api.SecretPatch _secretPatch({
    required String name,
    required String value,
    required bool configured,
  }) {
    if (value.isEmpty) {
      return rust_api.SecretPatch(
        name: name,
        kind: configured
            ? rust_api.SecretPatchKind.keep
            : rust_api.SecretPatchKind.clear,
      );
    }
    return rust_api.SecretPatch(
      name: name,
      value: value,
      kind: rust_api.SecretPatchKind.set_,
    );
  }

  int _parsePort(String text, String label, {bool allowZero = true}) {
    final value = text.trim().isEmpty ? 0 : int.tryParse(text.trim());
    if (value == null ||
        value < 0 ||
        value > 65535 ||
        (!allowZero && value == 0)) {
      throw FormatException('$label 无效');
    }
    return value;
  }

  List<String> _parseTags(String text) {
    final tags = <String>[];
    for (final rawTag in text.split(RegExp(r'[,，]'))) {
      final tag = rawTag.trim();
      if (tag.isEmpty) continue;
      if (tag.runes.length > 32) {
        throw const FormatException('单个标签不能超过 32 个字符');
      }
      if (tags.any((existing) => existing.toLowerCase() == tag.toLowerCase())) {
        continue;
      }
      tags.add(tag);
    }
    if (tags.length > 8) {
      throw const FormatException('最多只能添加 8 个标签');
    }
    return tags;
  }
}

class ParsedHeader {
  const ParsedHeader({required this.name, required this.value});

  final String name;
  final String value;
}

final _headerNamePattern = RegExp(r"^[!#$%&'*+\-.^_\x60|~0-9A-Za-z]+$");

List<ParsedHeader> parseHeaderClipboard(String input) {
  final lines = input
      .replaceAll('\r\n', '\n')
      .replaceAll('\r', '\n')
      .split('\n')
      .map((line) => line.trim())
      .where((line) => line.isNotEmpty)
      .toList();
  if (lines.isEmpty) {
    throw const FormatException('没有可解析的 header 内容');
  }

  final headers = <ParsedHeader>[];
  final names = <String>{};
  var index = 0;
  while (index < lines.length) {
    final line = lines[index];
    final separator = _headerSeparatorIndex(line);
    late final String name;
    late final String value;
    if (separator >= 0) {
      name = line.substring(0, separator).trim();
      value = line.substring(separator + 1).trim();
      index++;
    } else {
      name = line;
      if (index + 1 >= lines.length) {
        throw FormatException('header “$name” 缺少对应的值');
      }
      value = lines[index + 1];
      index += 2;
    }
    if (!_headerNamePattern.hasMatch(name)) {
      throw FormatException('header 名称无效: $name');
    }
    final normalizedName = name.toLowerCase();
    if (!names.add(normalizedName)) {
      throw FormatException('header 名称重复: $name');
    }
    headers.add(ParsedHeader(name: name, value: value));
  }
  return headers;
}

int _headerSeparatorIndex(String line) {
  final tab = line.indexOf('\t');
  if (tab > 0 && _headerNamePattern.hasMatch(line.substring(0, tab).trim())) {
    return tab;
  }
  final colon = line.indexOf(':');
  if (colon > 0 &&
      _headerNamePattern.hasMatch(line.substring(0, colon).trim())) {
    return colon;
  }
  return -1;
}

class _HeaderImportDialog extends StatefulWidget {
  const _HeaderImportDialog({required this.initialText});

  final String initialText;

  @override
  State<_HeaderImportDialog> createState() => _HeaderImportDialogState();
}

class _HeaderImportDialogState extends State<_HeaderImportDialog> {
  late final TextEditingController _controller;

  @override
  void initState() {
    super.initState();
    _controller = TextEditingController(text: widget.initialText);
    _controller.selection = TextSelection.collapsed(
      offset: _controller.text.length,
    );
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('批量导入 Headers'),
      content: SizedBox(
        width: 620,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Text(
              '支持“名称 + 下一行值”以及“名称: 值”格式，可直接粘贴浏览器复制的请求 Headers。',
              style: TextStyle(color: Color(0xFF60727D), fontSize: 12),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: _controller,
              minLines: 8,
              maxLines: 14,
              autofocus: true,
              decoration: const InputDecoration(
                hintText: 'sec-fetch-dest\nscript\nsec-fetch-mode: no-cors',
                alignLabelWithHint: true,
              ),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('取消'),
        ),
        FilledButton.icon(
          onPressed: () => Navigator.pop(context, _controller.text),
          icon: const Icon(Icons.playlist_add_check, size: 18),
          label: const Text('解析并导入'),
        ),
      ],
    );
  }
}

class AppSettingsDialog extends StatefulWidget {
  const AppSettingsDialog({super.key, required this.settings});

  final rust_api.AppSettingsView settings;

  @override
  State<AppSettingsDialog> createState() => _AppSettingsDialogState();
}

class _AppSettingsDialogState extends State<AppSettingsDialog> {
  late final TextEditingController _httpHost;
  late final TextEditingController _httpPort;
  late final TextEditingController _socksHost;
  late final TextEditingController _socksPort;
  late bool _httpEnabled;
  late bool _socksEnabled;
  String? _validationError;

  @override
  void initState() {
    super.initState();
    final http = widget.settings.httpProxy;
    final socks = widget.settings.socksProxy;
    _httpEnabled = http != null;
    _socksEnabled = socks != null;
    _httpHost = TextEditingController(text: http?.host ?? '127.0.0.1');
    _httpPort = TextEditingController(
      text: http == null ? '8080' : http.port.toString(),
    );
    _socksHost = TextEditingController(text: socks?.host ?? '127.0.0.1');
    _socksPort = TextEditingController(
      text: socks == null ? '8099' : socks.port.toString(),
    );
  }

  @override
  void dispose() {
    _httpHost.dispose();
    _httpPort.dispose();
    _socksHost.dispose();
    _socksPort.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Row(
        children: [
          Icon(Icons.tune_outlined, color: _brandTeal),
          SizedBox(width: 10),
          Text('应用设置'),
        ],
      ),
      content: SizedBox(
        width: 620,
        child: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const Text(
                '在这里统一配置出站代理。profile 只需选择是否使用对应的代理，所有服务仍然只监听 127.0.0.1。',
                style: TextStyle(color: Color(0xFF60727D), height: 1.4),
              ),
              const SizedBox(height: 18),
              _buildProxySection(
                title: 'HTTP 代理',
                subtitle: 'HTTP profile 访问上游 HTTP/HTTPS 服务时使用',
                icon: Icons.http_outlined,
                enabled: _httpEnabled,
                onChanged: (value) => setState(() => _httpEnabled = value),
                host: _httpHost,
                port: _httpPort,
              ),
              const SizedBox(height: 12),
              _buildProxySection(
                title: 'SOCKS5 代理',
                subtitle: 'SSH profile 建立 SSH 会话时使用，无认证，仅 TCP CONNECT',
                icon: Icons.hub_outlined,
                enabled: _socksEnabled,
                onChanged: (value) => setState(() => _socksEnabled = value),
                host: _socksHost,
                port: _socksPort,
              ),
              const SizedBox(height: 12),
              const Text(
                '修改代理地址后，已运行的 profile 需要停止并重新启动才会使用新设置。',
                style: TextStyle(color: Color(0xFF7A8B95), fontSize: 12),
              ),
              if (_validationError != null) ...[
                const SizedBox(height: 12),
                _ErrorBanner(message: _validationError!),
              ],
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('取消'),
        ),
        FilledButton.icon(
          onPressed: _save,
          icon: const Icon(Icons.save_outlined, size: 18),
          label: const Text('保存设置'),
        ),
      ],
    );
  }

  Widget _buildProxySection({
    required String title,
    required String subtitle,
    required IconData icon,
    required bool enabled,
    required ValueChanged<bool> onChanged,
    required TextEditingController host,
    required TextEditingController port,
  }) {
    return Container(
      padding: const EdgeInsets.fromLTRB(14, 12, 14, 14),
      decoration: BoxDecoration(
        color: _pageBackground,
        borderRadius: BorderRadius.circular(14),
        border: Border.all(color: _borderColor),
      ),
      child: Column(
        children: [
          SwitchListTile.adaptive(
            contentPadding: EdgeInsets.zero,
            secondary: Icon(icon, color: _brandTeal),
            title: Text(
              title,
              style: const TextStyle(fontWeight: FontWeight.w700),
            ),
            subtitle: Text(subtitle),
            value: enabled,
            onChanged: onChanged,
          ),
          if (enabled) ...[
            const SizedBox(height: 8),
            Row(
              children: [
                Expanded(
                  flex: 3,
                  child: TextField(
                    controller: host,
                    decoration: const InputDecoration(
                      labelText: '代理主机',
                      hintText: '127.0.0.1',
                    ),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: TextField(
                    controller: port,
                    keyboardType: TextInputType.number,
                    decoration: const InputDecoration(labelText: '端口'),
                  ),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }

  void _save() {
    try {
      final http = _httpEnabled
          ? _buildEndpoint(_httpHost, _httpPort, 'HTTP 代理')
          : null;
      final socks = _socksEnabled
          ? _buildEndpoint(_socksHost, _socksPort, 'SOCKS5 代理')
          : null;
      Navigator.pop(
        context,
        rust_api.AppSettingsDraft(httpProxy: http, socksProxy: socks),
      );
    } on FormatException catch (error) {
      setState(() => _validationError = error.message);
    }
  }

  rust_api.ProxyEndpoint _buildEndpoint(
    TextEditingController host,
    TextEditingController port,
    String label,
  ) {
    final hostText = host.text.trim();
    final portText = port.text.trim();
    final portNumber = int.tryParse(portText);
    if (hostText.isEmpty) {
      throw FormatException('$label主机不能为空');
    }
    if (hostText.contains(RegExp(r'\s')) ||
        hostText.contains('/') ||
        hostText.contains('?') ||
        hostText.contains('#') ||
        hostText.contains('@')) {
      throw FormatException('$label主机格式无效');
    }
    if (portNumber == null || portNumber < 1 || portNumber > 65535) {
      throw FormatException('$label端口无效');
    }
    return rust_api.ProxyEndpoint(host: hostText, port: portNumber);
  }
}

class _SecretRow {
  _SecretRow({String name = '', this.originalName, this.existing = false})
    : nameController = TextEditingController(text: name),
      valueController = TextEditingController();

  final TextEditingController nameController;
  final TextEditingController valueController;
  final String? originalName;
  final bool existing;

  void dispose() {
    nameController.dispose();
    valueController.dispose();
  }
}

class _ForwardRow {
  _ForwardRow({
    String localPort = '',
    String remoteHost = '',
    String remotePort = '',
    String tag = '',
  }) : localPort = TextEditingController(text: localPort),
       remoteHost = TextEditingController(text: remoteHost),
       remotePort = TextEditingController(text: remotePort),
       tag = TextEditingController(text: tag);

  final TextEditingController localPort;
  final TextEditingController remoteHost;
  final TextEditingController remotePort;
  final TextEditingController tag;

  void dispose() {
    localPort.dispose();
    remoteHost.dispose();
    remotePort.dispose();
    tag.dispose();
  }
}

class _PortField extends StatelessWidget {
  const _PortField({
    required this.controller,
    required this.label,
    required this.hint,
  });

  final TextEditingController controller;
  final String label;
  final String hint;

  @override
  Widget build(BuildContext context) {
    return TextFormField(
      controller: controller,
      keyboardType: TextInputType.number,
      decoration: InputDecoration(labelText: label, hintText: hint),
    );
  }
}

class _SecretTable extends StatelessWidget {
  const _SecretTable({
    required this.title,
    required this.hint,
    required this.rows,
    required this.onAdd,
    required this.onRemove,
    this.onImport,
  });

  final String title;
  final String hint;
  final List<_SecretRow> rows;
  final VoidCallback onAdd;
  final ValueChanged<_SecretRow> onRemove;
  final Future<void> Function()? onImport;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                title,
                style: const TextStyle(fontWeight: FontWeight.w700),
              ),
            ),
            if (onImport != null)
              TextButton.icon(
                onPressed: onImport,
                icon: const Icon(Icons.content_paste_go_outlined, size: 17),
                label: const Text('批量粘贴'),
              ),
            TextButton.icon(
              onPressed: onAdd,
              icon: const Icon(Icons.add, size: 17),
              label: const Text('添加'),
            ),
          ],
        ),
        if (rows.isEmpty)
          Text(
            '未配置，值将安全存储在 macOS Keychain',
            style: TextStyle(color: Colors.grey.shade600, fontSize: 12),
          )
        else
          ...rows.map(
            (row) => Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Row(
                children: [
                  Expanded(
                    child: TextField(
                      controller: row.nameController,
                      decoration: InputDecoration(labelText: hint),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: TextField(
                      controller: row.valueController,
                      obscureText: true,
                      decoration: InputDecoration(
                        labelText: row.existing ? '留空保留现有值' : '值',
                      ),
                    ),
                  ),
                  IconButton(
                    tooltip: '移除',
                    onPressed: () => onRemove(row),
                    icon: const Icon(Icons.remove_circle_outline),
                  ),
                ],
              ),
            ),
          ),
      ],
    );
  }
}

class _ForwardTable extends StatelessWidget {
  const _ForwardTable({
    required this.rows,
    required this.onAdd,
    required this.onRemove,
  });

  final List<_ForwardRow> rows;
  final VoidCallback onAdd;
  final ValueChanged<_ForwardRow> onRemove;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            const Expanded(
              child: Text(
                '本地端口转发',
                style: TextStyle(fontWeight: FontWeight.w700),
              ),
            ),
            TextButton.icon(
              onPressed: onAdd,
              icon: const Icon(Icons.add, size: 17),
              label: const Text('添加'),
            ),
          ],
        ),
        if (rows.isEmpty)
          Text(
            '例如：6380 → redis.example.com:6379',
            style: TextStyle(color: Colors.grey.shade600, fontSize: 12),
          )
        else ...[
          ...rows.map(
            (row) => Padding(
              padding: const EdgeInsets.only(top: 8),
              child: Row(
                children: [
                  Expanded(
                    child: TextField(
                      controller: row.localPort,
                      keyboardType: TextInputType.number,
                      decoration: const InputDecoration(labelText: '本地端口'),
                    ),
                  ),
                  const Padding(
                    padding: EdgeInsets.symmetric(horizontal: 8),
                    child: Icon(Icons.arrow_forward, size: 18),
                  ),
                  Expanded(
                    flex: 3,
                    child: TextField(
                      controller: row.remoteHost,
                      inputFormatters: [
                        FilteringTextInputFormatter.deny(RegExp(r'[\r\n]')),
                      ],
                      decoration: const InputDecoration(
                        labelText: '远端主机',
                        hintText: 'IP / 域名 / IPv6',
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: TextField(
                      controller: row.remotePort,
                      keyboardType: TextInputType.number,
                      decoration: const InputDecoration(labelText: '远端端口'),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: TextField(
                      controller: row.tag,
                      decoration: const InputDecoration(
                        labelText: 'Tag（可选）',
                        hintText: 'Redis',
                      ),
                    ),
                  ),
                  IconButton(
                    tooltip: '移除',
                    onPressed: () => onRemove(row),
                    icon: const Icon(Icons.remove_circle_outline),
                  ),
                ],
              ),
            ),
          ),
          const SizedBox(height: 7),
          const Text(
            '远端主机支持 IPv4、IPv6 和域名；域名由 SSH 服务器侧解析，端口请填写在右侧。',
            style: TextStyle(color: Color(0xFF71828C), fontSize: 11),
          ),
        ],
      ],
    );
  }
}

String? _endpointTag(
  rust_api.ProfileView profile,
  List<rust_api.Endpoint> endpoints,
  int endpointIndex,
) {
  final endpoint = endpoints[endpointIndex];
  if (endpoint.kind != 'sshForward' || profile.ssh == null) return null;
  final forwardIndex = endpoints
      .take(endpointIndex)
      .where((item) => item.kind == 'sshForward')
      .length;
  final forwards = profile.ssh!.forwards;
  if (forwardIndex >= forwards.length) return null;
  final tag = forwards[forwardIndex].tag.trim();
  return tag.isEmpty ? null : tag;
}

List<String> _mergeTags(List<String> tags, String? endpointTag) {
  final merged = [...tags];
  if (endpointTag != null &&
      !merged.any((tag) => tag.toLowerCase() == endpointTag.toLowerCase())) {
    merged.add(endpointTag);
  }
  return merged;
}

List<rust_api.Endpoint> _configuredEndpoints(rust_api.ProfileView profile) {
  if (profile.http != null) {
    final http = profile.http!;
    return [
      rust_api.Endpoint(
        kind: 'http',
        host: '127.0.0.1',
        port: http.listenPort,
        description: '固定上游反向转发 · ${http.upstreamUrl}',
      ),
    ];
  }
  final ssh = profile.ssh;
  if (ssh == null) return const [];
  return [
    ...ssh.forwards.map(
      (forward) => rust_api.Endpoint(
        kind: 'sshForward',
        host: '127.0.0.1',
        port: forward.localPort,
        description: '经 SSH 转发 → ${forward.remoteHost}:${forward.remotePort}',
      ),
    ),
  ];
}

String _endpointAddress(rust_api.Endpoint endpoint) {
  final hostPort = endpoint.port == 0
      ? '${endpoint.host}:启动后随机分配'
      : '${endpoint.host}:${endpoint.port}';
  return switch (endpoint.kind) {
    'http' => 'http://$hostPort',
    _ => hostPort,
  };
}

String _proxyDisplay(rust_api.ProxyEndpoint? endpoint) {
  if (endpoint == null) return '未配置（请先在应用设置中配置）';
  return _proxyAddress(endpoint);
}

String _proxyAddress(rust_api.ProxyEndpoint endpoint) {
  final host = endpoint.host;
  final displayHost = host.contains(':') && !host.startsWith('[')
      ? '[$host]'
      : host;
  return '$displayHost:${endpoint.port}';
}

String _endpointDetail(rust_api.Endpoint endpoint) {
  if (endpoint.kind == 'http') return '固定上游反向转发 · 仅监听本机回环地址';
  return endpoint.description.replaceFirst(
    RegExp(r'^SSH\s+127\.0\.0\.1:\d+\s+→\s+'),
    '远端目标 · ',
  );
}

String _endpointKindLabel(String kind) {
  return switch (kind) {
    'http' => 'HTTP 反向代理',
    _ => 'SSH 本地转发',
  };
}

bool _isActive(rust_api.ServiceState state) {
  return state == rust_api.ServiceState.starting ||
      state == rust_api.ServiceState.awaitingHostKey ||
      state == rust_api.ServiceState.running ||
      state == rust_api.ServiceState.reconnecting;
}

String _typeLabel(rust_api.ProxyType type) {
  return type == rust_api.ProxyType.http ? 'HTTP 反向代理' : 'SSH 隧道';
}

String _stateLabel(rust_api.ServiceState state) {
  return switch (state) {
    rust_api.ServiceState.stopped => '已停止',
    rust_api.ServiceState.starting => '启动中',
    rust_api.ServiceState.awaitingHostKey => '等待确认主机密钥',
    rust_api.ServiceState.running => '运行中',
    rust_api.ServiceState.reconnecting => '自动重连中',
    rust_api.ServiceState.error => '错误',
  };
}

Color _stateColor(rust_api.ServiceState state) {
  return switch (state) {
    rust_api.ServiceState.stopped => Colors.grey,
    rust_api.ServiceState.starting => Colors.blue,
    rust_api.ServiceState.awaitingHostKey => Colors.orange,
    rust_api.ServiceState.running => Colors.green.shade700,
    rust_api.ServiceState.reconnecting => Colors.orange.shade800,
    rust_api.ServiceState.error => Colors.red.shade700,
  };
}

IconData _endpointIcon(String kind) {
  return switch (kind) {
    'http' => Icons.http_outlined,
    _ => Icons.swap_horiz_outlined,
  };
}
