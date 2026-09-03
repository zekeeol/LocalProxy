// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'local_proxy.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$StartOutcome {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is StartOutcome);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'StartOutcome()';
}


}

/// @nodoc
class $StartOutcomeCopyWith<$Res>  {
$StartOutcomeCopyWith(StartOutcome _, $Res Function(StartOutcome) __);
}


/// Adds pattern-matching-related methods to [StartOutcome].
extension StartOutcomePatterns on StartOutcome {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( StartOutcome_Started value)?  started,TResult Function( StartOutcome_AlreadyRunning value)?  alreadyRunning,required TResult orElse(),}){
final _that = this;
switch (_that) {
case StartOutcome_Started() when started != null:
return started(_that);case StartOutcome_AlreadyRunning() when alreadyRunning != null:
return alreadyRunning(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( StartOutcome_Started value)  started,required TResult Function( StartOutcome_AlreadyRunning value)  alreadyRunning,}){
final _that = this;
switch (_that) {
case StartOutcome_Started():
return started(_that);case StartOutcome_AlreadyRunning():
return alreadyRunning(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( StartOutcome_Started value)?  started,TResult? Function( StartOutcome_AlreadyRunning value)?  alreadyRunning,}){
final _that = this;
switch (_that) {
case StartOutcome_Started() when started != null:
return started(_that);case StartOutcome_AlreadyRunning() when alreadyRunning != null:
return alreadyRunning(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  started,TResult Function( List<Endpoint> endpoints)?  alreadyRunning,required TResult orElse(),}) {final _that = this;
switch (_that) {
case StartOutcome_Started() when started != null:
return started();case StartOutcome_AlreadyRunning() when alreadyRunning != null:
return alreadyRunning(_that.endpoints);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  started,required TResult Function( List<Endpoint> endpoints)  alreadyRunning,}) {final _that = this;
switch (_that) {
case StartOutcome_Started():
return started();case StartOutcome_AlreadyRunning():
return alreadyRunning(_that.endpoints);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  started,TResult? Function( List<Endpoint> endpoints)?  alreadyRunning,}) {final _that = this;
switch (_that) {
case StartOutcome_Started() when started != null:
return started();case StartOutcome_AlreadyRunning() when alreadyRunning != null:
return alreadyRunning(_that.endpoints);case _:
  return null;

}
}

}

/// @nodoc


class StartOutcome_Started extends StartOutcome {
  const StartOutcome_Started(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is StartOutcome_Started);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'StartOutcome.started()';
}


}




/// @nodoc


class StartOutcome_AlreadyRunning extends StartOutcome {
  const StartOutcome_AlreadyRunning({required final  List<Endpoint> endpoints}): _endpoints = endpoints,super._();
  

 final  List<Endpoint> _endpoints;
 List<Endpoint> get endpoints {
  if (_endpoints is EqualUnmodifiableListView) return _endpoints;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_endpoints);
}


/// Create a copy of StartOutcome
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$StartOutcome_AlreadyRunningCopyWith<StartOutcome_AlreadyRunning> get copyWith => _$StartOutcome_AlreadyRunningCopyWithImpl<StartOutcome_AlreadyRunning>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is StartOutcome_AlreadyRunning&&const DeepCollectionEquality().equals(other._endpoints, _endpoints));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_endpoints));

@override
String toString() {
  return 'StartOutcome.alreadyRunning(endpoints: $endpoints)';
}


}

/// @nodoc
abstract mixin class $StartOutcome_AlreadyRunningCopyWith<$Res> implements $StartOutcomeCopyWith<$Res> {
  factory $StartOutcome_AlreadyRunningCopyWith(StartOutcome_AlreadyRunning value, $Res Function(StartOutcome_AlreadyRunning) _then) = _$StartOutcome_AlreadyRunningCopyWithImpl;
@useResult
$Res call({
 List<Endpoint> endpoints
});




}
/// @nodoc
class _$StartOutcome_AlreadyRunningCopyWithImpl<$Res>
    implements $StartOutcome_AlreadyRunningCopyWith<$Res> {
  _$StartOutcome_AlreadyRunningCopyWithImpl(this._self, this._then);

  final StartOutcome_AlreadyRunning _self;
  final $Res Function(StartOutcome_AlreadyRunning) _then;

/// Create a copy of StartOutcome
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? endpoints = null,}) {
  return _then(StartOutcome_AlreadyRunning(
endpoints: null == endpoints ? _self._endpoints : endpoints // ignore: cast_nullable_to_non_nullable
as List<Endpoint>,
  ));
}


}

// dart format on
