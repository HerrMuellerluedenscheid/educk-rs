import 'dart:convert';
import 'package:http/http.dart' as http;
import '../models/energy_data.dart';

class ApiService {
  final String baseUrl;

  const ApiService({required this.baseUrl});

  Future<EnergyData> fetchEnergyData(String country, {int hours = 24}) async {
    final uri = Uri.parse(
        '$baseUrl/api/v1/renewable-surplus/$country/plot-json?hours=$hours');
    final response =
        await http.get(uri).timeout(const Duration(seconds: 30));

    if (response.statusCode != 200) {
      throw Exception('Server returned HTTP ${response.statusCode}');
    }

    final json = jsonDecode(response.body) as Map<String, dynamic>;
    if (json['success'] != true || json['data'] == null) {
      throw Exception(json['error'] ?? 'No data returned by server');
    }

    return EnergyData.fromJson(json, country);
  }

  Future<List<String>> fetchCountries() async {
    final uri = Uri.parse('$baseUrl/api/v1/countries');
    final response =
        await http.get(uri).timeout(const Duration(seconds: 10));

    if (response.statusCode != 200) {
      throw Exception('Server returned HTTP ${response.statusCode}');
    }

    final json = jsonDecode(response.body) as Map<String, dynamic>;
    return ((json['data'] as List).cast<String>())..sort();
  }
}
